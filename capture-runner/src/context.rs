//! Per-attempt native transport. No retries, no proxies, raw bytes before parsing.
use crate::{
    journal::Journal,
    model::{canonical_provider, transport_error, unknown_event},
};
use anyhow::{Context, Result, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use futures_util::{SinkExt, StreamExt};
use reqwest::{
    cookie::{CookieStore, Jar},
    header::{HeaderMap, HeaderName, HeaderValue, SET_COOKIE},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tokio::{
    net::TcpStream,
    time::{Instant, timeout},
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{Message, client::IntoClientRequest, protocol::WebSocketConfig},
};
use url::Url;
const MAX_BODY: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct WireResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: String,
    pub set_cookies: Vec<String>,
}
pub struct NativeSocket {
    stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
    url: String,
}
#[derive(Clone, Debug)]
pub enum SocketPayload {
    Text(String),
    Binary(Vec<u8>),
}
pub struct DecodedFrame {
    pub values: Vec<Value>,
    pub replies: Vec<SocketPayload>,
}
#[derive(Clone)]
pub struct RunContext {
    pub journal: Journal,
    provider: String,
    client: reqwest::Client,
    cookies: Arc<Jar>,
    deadline: Duration,
    steps: usize,
    #[cfg(test)]
    test_origin: Option<String>,
}
fn headers(source: &HeaderMap) -> Vec<(String, String)> {
    source
        .iter()
        .map(|(k, v)| (k.to_string(), STANDARD.encode(v.as_bytes())))
        .collect()
}
fn wire_response(status: u16, source: &HeaderMap, bytes: Vec<u8>) -> WireResponse {
    let mut headers = source
        .iter()
        .map(|(k, v)| {
            (
                k.to_string(),
                v.to_str().unwrap_or("[binary header]").to_owned(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let body = match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(e) => {
            headers.insert("x-evidencia-body-encoding".into(), "base64".into());
            STANDARD.encode(e.into_bytes())
        }
    };
    WireResponse {
        status,
        headers,
        body,
        set_cookies: source
            .get_all(SET_COOKIE)
            .iter()
            .map(|v| v.to_str().unwrap_or("").to_owned())
            .collect(),
    }
}
impl RunContext {
    pub fn new(provider: &str, journal: Journal, deadline: Duration, steps: usize) -> Result<Self> {
        let cookies = Arc::new(Jar::default());
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .cookie_provider(cookies.clone())
            .timeout(deadline)
            .connect_timeout(deadline)
            .build()?;
        Ok(Self {
            journal,
            provider: provider.into(),
            client,
            cookies,
            deadline,
            steps,
            #[cfg(test)]
            test_origin: None,
        })
    }
    pub fn max_steps(&self) -> usize {
        self.steps
    }
    pub fn user_agent(&self) -> &'static str {
        match canonical_provider(&self.provider) {
            "endorphina" | "mascot" | "habanero" => "Mozilla/5.0",
            "peterandsons" => "PeterAndSons-Demo-QA/1.0",
            _ => "Evidencia-Capture-Runner/0.1",
        }
    }
    pub fn save_session(&self, owner: &Value) -> Result<()> {
        self.note("session", owner)
    }
    pub fn note(&self, event: &str, data: &Value) -> Result<()> {
        self.journal.record(event, data)?;
        Ok(())
    }
    fn url(&self, value: &str) -> Result<Url> {
        #[cfg(test)]
        if self.test_origin.as_deref().is_some_and(|origin| {
            Url::parse(value).is_ok_and(|u| u.origin().ascii_serialization() == origin)
        }) {
            return Ok(Url::parse(value)?);
        }
        validate_target(&self.provider, value)
    }
    pub fn seed_cookie(&self, cookie: &str, url: &str) -> Result<()> {
        let url = self.url(url)?;
        self.cookies.add_cookie_str(cookie, &url);
        Ok(())
    }
    pub async fn get(&self, label: &str, url: &str) -> Result<WireResponse> {
        self.http(label, "GET", url, BTreeMap::new(), String::new(), false)
            .await
    }
    pub async fn http(
        &self,
        label: &str,
        method: &str,
        url: &str,
        headers: BTreeMap<String, String>,
        body: String,
        round_action: bool,
    ) -> Result<WireResponse> {
        let target = self.url(url)?;
        ensure!(
            matches!(target.scheme(), "https") || (cfg!(test) && target.scheme() == "http"),
            "HTTP requires HTTPS"
        );
        ensure!(body.len() <= MAX_BODY, "request exceeds 16 MiB");
        let cookie = self.cookies.cookies(&target);
        let mut builder = self
            .client
            .request(method.parse()?, target)
            .header("User-Agent", self.user_agent());
        if !headers.keys().any(|h| h.eq_ignore_ascii_case("cookie"))
            && let Some(cookie) = cookie
        {
            builder = builder.header("Cookie", cookie);
        }
        for (k, v) in &headers {
            builder = builder.header(k, v);
        }
        let request = builder.body(body).build()?;
        let id=self.journal.record("http_request",&json!({"label":label,"method":method,"url":url,"headers_base64":crate::context::headers(request.headers()),"body_base64":STANDARD.encode(request.body().and_then(|b|b.as_bytes()).unwrap_or_default()),"round_action":round_action}))?;
        let result:Result<WireResponse>=async {
            let mut response=self.client.execute(request).await.map_err(|e|transport_error(format!("HTTP request failed: {}",e.without_url())))?;
            let status=response.status().as_u16();let response_headers=response.headers().clone();
            self.journal.record("http_response_headers",&json!({"request_seq":id,"status":status,"headers_base64":crate::context::headers(&response_headers)}))?;
            let mut bytes=Vec::new();
            while let Some(chunk)=response.chunk().await.map_err(|e|transport_error(format!("HTTP body incomplete: {}",e.without_url())))? {
                self.journal.record("http_response_chunk",&json!({"request_seq":id,"body_base64":STANDARD.encode(&chunk)}))?;
                ensure!(bytes.len().saturating_add(chunk.len())<=MAX_BODY,"HTTP response exceeds 16 MiB; captured prefix retained");bytes.extend_from_slice(&chunk);
            }
            self.journal.record("http_response_end",&json!({"request_seq":id,"bytes":bytes.len()}))?;
            Ok(wire_response(status,&response_headers,bytes))
        }.await;
        if let Err(error) = &result {
            self.journal.record(
                "transport_error",
                &json!({"request_seq":id,"error":format!("{error:#}")}),
            )?;
        }
        result
    }
    pub async fn json(
        &self,
        label: &str,
        url: &str,
        body: &Value,
        mut headers: BTreeMap<String, String>,
        round_action: bool,
    ) -> Result<Value> {
        headers
            .entry("content-type".into())
            .or_insert_with(|| "application/json".into());
        let response = self
            .http(
                label,
                "POST",
                url,
                headers,
                serde_json::to_string(body)?,
                round_action,
            )
            .await?;
        ensure!(
            (200..300).contains(&response.status),
            "{label}: provider HTTP {}",
            response.status
        );
        serde_json::from_str(&response.body)
            .map_err(|e| unknown_event(format!("{label}: expected JSON: {e}")))
    }
    pub async fn websocket(
        &self,
        url: &str,
        origin: &str,
        subprotocol: Option<&str>,
        headers: BTreeMap<String, String>,
    ) -> Result<NativeSocket> {
        let target = self.url(url)?;
        self.url(origin)?;
        let mut request = target.as_str().into_client_request()?;
        request.headers_mut().insert("Origin", origin.parse()?);
        request
            .headers_mut()
            .insert("User-Agent", self.user_agent().parse()?);
        if let Some(value) = subprotocol {
            request
                .headers_mut()
                .insert("Sec-WebSocket-Protocol", value.parse()?);
        }
        let mut cookie_url = target.clone();
        cookie_url
            .set_scheme("https")
            .map_err(|_| anyhow::anyhow!("cookie scheme"))?;
        if let Some(cookie) = self.cookies.cookies(&cookie_url) {
            request.headers_mut().insert("Cookie", cookie);
        }
        for (k, v) in headers {
            request.headers_mut().insert(
                HeaderName::from_bytes(k.as_bytes())?,
                HeaderValue::from_str(&v)?,
            );
        }
        let id = self.journal.record(
            "ws_handshake_request",
            &json!({"url":url,"headers_base64":crate::context::headers(request.headers())}),
        )?;
        let config = WebSocketConfig::default()
            .max_message_size(Some(MAX_BODY))
            .max_frame_size(Some(MAX_BODY));
        match timeout(
            self.deadline,
            tokio_tungstenite::connect_async_with_config(request, Some(config), true),
        )
        .await
        {
            Ok(Ok((stream, response))) => {
                self.journal.record("ws_handshake_response",&json!({"request_seq":id,"status":response.status().as_u16(),"headers_base64":crate::context::headers(response.headers())}))?;
                self.cookies.set_cookies(
                    &mut response.headers().get_all(SET_COOKIE).iter(),
                    &cookie_url,
                );
                Ok(NativeSocket {
                    stream,
                    url: url.into(),
                })
            }
            Ok(Err(error)) => {
                if let tokio_tungstenite::tungstenite::Error::Http(response) = &error {
                    self.journal.record("ws_handshake_response",&json!({"request_seq":id,"status":response.status().as_u16(),"headers_base64":crate::context::headers(response.headers()),"buffered_body_base64":STANDARD.encode(response.body().as_deref().unwrap_or_default())}))?;
                }
                self.note(
                    "transport_error",
                    &json!({"request_seq":id,"error":error.to_string()}),
                )?;
                Err(transport_error(
                    "WebSocket handshake failed; see wire journal",
                ))
            }
            Err(_) => {
                self.note(
                    "transport_error",
                    &json!({"request_seq":id,"error":"handshake timeout"}),
                )?;
                Err(transport_error("WebSocket handshake timed out"))
            }
        }
    }
    pub async fn ws_codec<F>(
        &self,
        socket: &mut NativeSocket,
        label: &str,
        payload: SocketPayload,
        round_action: bool,
        request_id: Option<&str>,
        mut decode: F,
    ) -> Result<Value>
    where
        F: FnMut(&SocketPayload) -> Result<Vec<Value>> + Send,
    {
        self.ws_codec_controlled(
            socket,
            label,
            payload,
            round_action,
            request_id,
            move |frame| {
                Ok(DecodedFrame {
                    values: decode(frame)?,
                    replies: vec![],
                })
            },
        )
        .await
    }
    pub async fn ws_codec_controlled<F>(
        &self,
        socket: &mut NativeSocket,
        label: &str,
        payload: SocketPayload,
        round_action: bool,
        request_id: Option<&str>,
        decode: F,
    ) -> Result<Value>
    where
        F: FnMut(&SocketPayload) -> Result<DecodedFrame> + Send,
    {
        self.ws_exchange(
            socket,
            label,
            Some(payload),
            round_action,
            request_id,
            decode,
        )
        .await
    }
    async fn send_frame(
        &self,
        socket: &mut NativeSocket,
        label: &str,
        payload: SocketPayload,
        round_action: bool,
        remaining: Duration,
    ) -> Result<()> {
        let (kind, raw, message) = match payload {
            SocketPayload::Text(s) => ("text", s.clone(), Message::Text(s.into())),
            SocketPayload::Binary(b) => (
                "binary_base64",
                STANDARD.encode(&b),
                Message::Binary(b.into()),
            ),
        };
        ensure!(raw.len() <= MAX_BODY * 2, "outgoing frame too large");
        self.note("ws_send",&json!({"label":label,"url":socket.url,"kind":kind,"raw":raw,"round_action":round_action}))?;
        timeout(remaining, socket.stream.send(message))
            .await
            .map_err(|_| transport_error("WSS send timed out"))?
            .map_err(|e| transport_error(format!("WSS send: {e}")))?;
        Ok(())
    }
    pub async fn ws_exchange<F>(
        &self,
        socket: &mut NativeSocket,
        label: &str,
        payload: Option<SocketPayload>,
        round_action: bool,
        request_id: Option<&str>,
        mut decode: F,
    ) -> Result<Value>
    where
        F: FnMut(&SocketPayload) -> Result<DecodedFrame> + Send,
    {
        let deadline = Instant::now() + self.deadline;
        if let Some(payload) = payload {
            self.send_frame(socket, label, payload, round_action, self.deadline)
                .await?;
        }
        let mut received = 0usize;
        for _ in 0..1000 {
            let frame = timeout(
                deadline.saturating_duration_since(Instant::now()),
                socket.stream.next(),
            )
            .await
            .map_err(|_| transport_error("WSS receive timed out"))?
            .ok_or_else(|| transport_error("WSS closed before response"))?
            .map_err(|e| transport_error(format!("WSS receive: {e}")))?;
            let (kind, raw) = match &frame {
                Message::Text(s) => ("text", s.to_string()),
                Message::Binary(b) => ("binary_base64", STANDARD.encode(b)),
                Message::Ping(b) => ("ping_base64", STANDARD.encode(b)),
                Message::Pong(b) => ("pong_base64", STANDARD.encode(b)),
                other => ("control", format!("{other:?}")),
            };
            self.note(
                "ws_receive",
                &json!({"label":label,"url":socket.url,"kind":kind,"raw":raw}),
            )?;
            received += raw.len();
            ensure!(received <= MAX_BODY * 2, "WSS exchange size limit");
            let native = match frame {
                Message::Text(t) => SocketPayload::Text(t.to_string()),
                Message::Binary(b) => SocketPayload::Binary(b.to_vec()),
                Message::Ping(b) => {
                    self.note("ws_pong", &json!({"raw_base64":STANDARD.encode(&b)}))?;
                    timeout(
                        deadline.saturating_duration_since(Instant::now()),
                        socket.stream.send(Message::Pong(b)),
                    )
                    .await
                    .map_err(|_| transport_error("WSS pong timeout"))??;
                    continue;
                }
                Message::Pong(_) => continue,
                Message::Close(_) => {
                    return Err(transport_error("WSS peer closed before native completion"));
                }
                _ => return Err(unknown_event("unexpected WebSocket control frame")),
            };
            let decoded = decode(&native).with_context(|| format!("{label}: native frame"))?;
            for reply in decoded.replies {
                self.send_frame(
                    socket,
                    label,
                    reply,
                    false,
                    deadline.saturating_duration_since(Instant::now()),
                )
                .await?;
            }
            for value in decoded.values {
                if request_id.is_none_or(|id| {
                    value
                        .get("requestId")
                        .or_else(|| value.get("id"))
                        .is_some_and(|v| {
                            v.as_str()
                                .map(str::to_owned)
                                .unwrap_or_else(|| v.to_string())
                                == id
                        })
                }) {
                    return Ok(value);
                }
                self.note("ws_uncorrelated", &json!({"label":label,"value":value}))?;
            }
        }
        Err(unknown_event("no correlated response within 1000 frames"))
    }
    pub async fn ws_expect_text(
        &self,
        socket: &mut NativeSocket,
        label: &str,
        expected: &str,
    ) -> Result<()> {
        self.ws_exchange(socket, label, None, false, None, |frame| {
            if !matches!(frame,SocketPayload::Text(s)if s==expected) {
                return Err(unknown_event("unexpected native greeting"));
            }
            Ok(DecodedFrame {
                values: vec![json!(expected)],
                replies: vec![],
            })
        })
        .await?;
        Ok(())
    }
    pub async fn ws_text_decoded(
        &self,
        socket: &mut NativeSocket,
        label: &str,
        payload: &str,
        round_action: bool,
        request_id: Option<&str>,
        decode: fn(&str) -> Result<Value>,
    ) -> Result<Value> {
        self.ws_codec(
            socket,
            label,
            SocketPayload::Text(payload.into()),
            round_action,
            request_id,
            |frame| match frame {
                SocketPayload::Text(s) => Ok(vec![decode(s)?]),
                _ => Err(unknown_event("expected native text frame")),
            },
        )
        .await
    }
    pub async fn ws_json(
        &self,
        socket: &mut NativeSocket,
        label: &str,
        request: &Value,
        round_action: bool,
        request_id: Option<&str>,
    ) -> Result<Value> {
        self.ws_codec(
            socket,
            label,
            SocketPayload::Text(serde_json::to_string(request)?),
            round_action,
            request_id,
            |frame| match frame {
                SocketPayload::Text(s) => Ok(vec![serde_json::from_str(s)?]),
                _ => Err(unknown_event("expected JSON text frame")),
            },
        )
        .await
    }
}

// Demo endpoint evidence from standalone commit 749ae8e (no legacy runtime dependency).
pub fn validate_target(provider: &str, value: &str) -> Result<Url> {
    let url = Url::parse(value).context("invalid provider URL")?;
    ensure!(
        matches!(url.scheme(), "https" | "wss"),
        "only TLS demo endpoints are allowed"
    );
    ensure!(
        url.username().is_empty() && url.password().is_none(),
        "credentials in provider URLs are forbidden"
    );
    ensure!(
        url.port().is_none_or(|port| port == 443)
            || (provider == "1spin4win" && value == "https://gs.1spin4win.com:10443"),
        "unexpected provider port"
    );
    ensure!(
        url.fragment().is_none(),
        "provider URL fragments are forbidden"
    );
    let host = url.host_str().context("provider host missing")?;
    let allowed = match canonical_provider(provider) {
        "bgaming" => {
            (matches!(host, "demo.bgaming-network.com" | "cdn.bgaming-network.com")
                || host.ends_with(".demo.bgaming-network.com"))
                && url.scheme() == "https"
        }
        "smartsoft" => {
            (host == "www.smartsoftgaming.com"
                || host == "demos.smartsoftgaming.com"
                || host.ends_with(".ssgportal.com"))
                && matches!(url.scheme(), "https" | "wss")
        }
        "threeoaks" => {
            (host == "3oaks.com" || host.ends_with(".3oaks.com")) && url.scheme() == "https"
        }
        "rubyplay" => {
            url.scheme() == "https"
                && match host {
                    "static2.eu3cdn-83.servpavloro.com" => {
                        url.path() == "/africancats/latest/static/js/game.js"
                            && url.query().is_none()
                    }
                    "srv.prrpeu3.com" => {
                        url.path() == "/init-session/demo"
                            || (url.path() == "/gameserver/demo" && url.query().is_none())
                    }
                    _ => false,
                }
        }
        "kendoo" => {
            url.scheme() == "https"
                && match host {
                    "kendoo.com" => {
                        url.query().is_none()
                            && url
                                .path()
                                .strip_prefix("/games/")
                                .is_some_and(crate::adapters::kendoo::known_slug)
                    }
                    "betman-demo.head.kendoo.pro" => {
                        url.path() == "/betman-demo/static/game.html"
                            || (url.query().is_none()
                                && url.path().ends_with("/demo/")
                                && crate::adapters::kendoo::native_codes().any(|code| {
                                    url.path()
                                        .starts_with(&format!("/betman-demo/gs/{code}/desktop/"))
                                }))
                    }
                    _ => false,
                }
        }
        "playtech" => match (host, url.scheme()) {
            ("gpas-games.playtechone.com", "https") => {
                url.query().is_none()
                    && (matches!(url.path(), "/" | "/gpasclientconf_live.php")
                        || url.path().ends_with("/logical_bundle.php")
                        || url
                            .path()
                            .ends_with("/brands/playtechhorizon/1/json/env_config.json")
                        || (url
                            .path()
                            .starts_with("/gpasdistributed/igv/gpas_blitz10_pop/")
                            && url.path().ends_with("/packs/manifest.json")))
            }
            ("gpas-stg.playtechone.com" | "frontend.playtechhorizon.com", "https") => {
                url.path() == "/socket.io/1/"
            }
            ("gpas-stg.playtechone.com" | "frontend.playtechhorizon.com", "wss") => {
                url.query().is_none() && url.path().starts_with("/socket.io/1/websocket/")
            }
            _ => false,
        },
        "gamesglobal" => {
            url.scheme() == "https"
                && match host {
                    "www.gamesglobal.com" => url.path().starts_with("/api/launch/"),
                    "redirector32.valueactive.eu" | "redirector32.valueactive.com" => {
                        url.path() == "/Casino/Default.aspx"
                    }
                    "gamelauncheruat.gameassists.co.uk" | "gamelauncher.gameassists.co.uk" => {
                        url.path() == "/launcher/Generic"
                    }
                    "mobile32.gameassists.co.uk" | "mobile32.gameassists.com" => {
                        url.path() == "/MobileWebServices_40/casino/game/launch"
                            || url.path().starts_with("/htmlgames/game/")
                    }
                    "api32.gameassists.co.uk" => matches!(
                        url.path(),
                        "/casino/user/public/v1/accounts/login/demo"
                            | "/Shared/Communications/Public/v1/accounts/authenticate"
                            | "/casino/play/public/v1/games/module/101053/client/50300/play"
                            | "/casino/play/public/v1/games/module/101417/client/50300/play"
                    ),
                    _ => false,
                }
        }
        "habanero" => {
            url.scheme() == "https"
                && match host {
                    "gs-test.insvr.com" => {
                        url.path() == "/pf" && url.query() == Some("client=1.1.24603.707")
                    }
                    "app-test.insvr.com" => {
                        url.query().is_none()
                            && url.path().starts_with("/gamecontent/")
                            && url
                                .path()
                                .rsplit_once("/js/script0.")
                                .is_some_and(|(_, version)| {
                                    version.strip_suffix(".min.js").is_some_and(|v| {
                                        !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit())
                                    })
                                })
                    }
                    _ => false,
                }
        }
        "mascot" => {
            url.scheme() == "https"
                && host.strip_suffix(".mascot.games").is_some_and(|subdomain| {
                    !subdomain.is_empty()
                        && subdomain
                            .bytes()
                            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                        && (host != "demo.mascot.games" || url.path().starts_with("/run/"))
                })
        }
        "spinomenal" => {
            url.scheme() == "https"
                && url.query().is_none()
                && match host {
                    "rgs-ld-demo.spinomenal.com" => url.path() == "/api/InitClientUrl",
                    "rgs-ld-demo-core.spinomenal.com" => matches!(
                        url.path(),
                        "/api/StartSlotMachine"
                            | "/api/ProcessSlotMachine"
                            | "/api/BuyFeature"
                            | "/api/Gamble"
                            | "/api/PickFreeSpins"
                    ),
                    _ => false,
                }
        }
        "amigogaming" => match (host, url.scheme()) {
            ("amigogaming.com", "https") => {
                matches!(url.path(), "/es/games" | "/es/generateDemoGameLink")
            }
            ("launch.amigogaming.com", "https") => url.path().starts_with("/session/open/sid/"),
            ("demo.amigogaming.online", "https") => url.path() == "//icasino/sessions/launch",
            ("demo.amigogaming.cloud", "https") => {
                matches!(
                    url.path(),
                    "/" | "//icasino/sessions/launch"
                        | "/organic2/websocket/launch"
                        | "/js-frontend/html5/index.html"
                ) || url.path().starts_with("/games-alt-wgl/amigo/")
            }
            ("sdemo.amigogaming.cloud", "wss") => url.path() == "/organic2/websocket/endpoint/2",
            _ => false,
        },
        "irondog" => {
            url.scheme() == "https"
                && match host {
                    "gamelauncherservice-stage.1x2networkhub.com"
                    | "gamelauncherservice.desertorchidhub.com" => {
                        url.path() == "/" && url.query().is_none()
                    }
                    "nogs-gl-stage.nyxmalta.com" => url.path() == "/game/",
                    "gp.sociumhubeurope.com" | "gp-stage.1x2hubeurope.com" => matches!(
                        url.path(),
                        "/f1x2games/getGuestID.jsp" | "/gateway/init" | "/gateway/"
                    ),
                    _ => false,
                }
        }
        "thunderspin" => {
            url.scheme() == "https"
                && match host {
                    "cu.thunderspin.net" | "stage-cu.thunderspin.net" => {
                        url.path().starts_with("/games/") || url.path().starts_with("/service/")
                    }
                    "games.thunderspin.com" => [
                        "/monstersushi/",
                        "/necronomicon/",
                        "/pyramids/",
                        "/bookofgames/",
                        "/service/",
                    ]
                    .iter()
                    .any(|prefix| url.path().starts_with(prefix)),
                    "expo.bossgs.net" => url.path().starts_with("/service/"),
                    _ => false,
                }
        }
        "1spin4win" => {
            value == "https://gs.1spin4win.com:10443"
                || (host == "gs.1spin4win.com"
                    && url.scheme() == "wss"
                    && url.path() == "/games"
                    && url.query().is_none())
        }
        "egt-digital" => matches!(
            (host, url.scheme()),
            ("games.egt-ong.com", "https") | ("game-server-demo.egt-ong.com", "wss")
        ),
        "betsoft" => {
            url.scheme() == "https"
                && match host {
                    "democasino.betsoftgaming.com" | "lobby-gp3demo.betsoftgaming.com" => {
                        matches!(url.path(), "/cwguestlogin.do" | "/free/base/template.jsp")
                    }
                    "games-gp3demo.betsoftgaming.com" => {
                        url.query().is_none()
                            && url
                                .path()
                                .strip_prefix('/')
                                .and_then(|path| path.strip_suffix(".game"))
                                .is_some_and(|game| {
                                    !game.is_empty()
                                        && game.bytes().all(|b| {
                                            b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_')
                                        })
                                })
                    }
                    _ => false,
                }
        }
        "evoplay" => {
            url.scheme() == "https"
                && (host.ends_with(".evoplay.games") || host.ends_with(".demo-evoplay.games"))
        }
        "ela-games" => match (host, url.scheme()) {
            ("expo.elaapi.com", "https") => {
                url.path().starts_with("/demo/") && url.query().is_none()
            }
            ("preprod.elaapi.com", "https") => matches!(url.path(), "/" | "/launcher/"),
            ("preprod.elaapi.com", "wss") => url.path() == "/gateway-game/socket.io/",
            _ => false,
        },
        "amusnet" => {
            matches!(
                (host, url.scheme(), url.path()),
                ("amusnet.com", "https", "/api/lobby")
                    | ("free-pod.games.amusnet.io", "https", "/demo/v1/launch")
                    | ("prod.cdn.amusnet.io", "https", "/")
            ) || (url.scheme() == "wss"
                && url.path() == "/"
                && crate::adapters::amusnet::demo_socket_host(host))
        }
        "hacksaw" => matches!(
            host,
            "static-live.hacksawgaming.com" | "rgs-demo.hacksawgaming.com"
        ),
        "endorphina" => matches!(
            host,
            "endorphina.com"
                | "edemo.endorphina.com"
                | "demo.endorphina.network"
                | "dyndemo.endorphina.network"
        ),
        "peterandsons" => matches!(host, "demo.spinart.cloud" | "cdn-demo.spinart.cloud"),
        "nolimit" => matches!(host, "demo.nolimitcdn.com" | "demo.nolimitcity.com"),
        "hardfire-kalamba" => {
            (matches!(
                host,
                "kalamba.integration.demo.kalambagames.com"
                    | "v.play.test.kl.luckycdn.net"
                    | "v.games.test.kl.luckycdn.net"
            ) && url.scheme() == "https")
                || (host == "awscdn.test.kalamba.net"
                    && url.scheme() == "https"
                    && matches!(url.path(), "/" | "/index.html"))
                || (host
                    .strip_prefix("gameserver-api-")
                    .and_then(|s| s.strip_suffix(".test.twmc8fvg.click"))
                    .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
                    && url.scheme() == "wss"
                    && url.path() == "/oryxreplica/")
        }
        "yggdrasil" => matches!(
            host,
            "demo.yggdrasilgaming.com" | "staticdemo.yggdrasilgaming.com"
        ),
        "belatra" => url.scheme() == "https" && host == "demo.bltrm.com",
        "microgaming" => {
            host == "api8888.maf-114.xyz"
                && url.scheme() == "https"
                && url.path().starts_with("/base/")
        }
        "elk" => matches!(
            host,
            "gamelauncher-stage.contentmedia.eu"
                | "static-stage.contentmedia.eu"
                | "gc5-stage.contentmedia.eu"
        ),
        "kagaming" => matches!(host, "gamesdemo.kaga88.com" | "rmpdemo.kaga88.com"),
        "playngo" => matches!(
            host,
            "www.playngo.com"
                | "released.playngonetwork.com"
                | "ascflyp.playngonetwork.com"
                | "cdn.playngonetwork.com"
        ),
        "netent" => match (url.scheme(), host) {
            ("wss", "showcase.evo-games.com") => url.path() == "/public/horizon",
            ("https", "games.evolution.com") => {
                url.path().starts_with("/slots/") || url.path() == "/wp-json/games/v1/start"
            }
            ("https", "showcase.evo-games.com") => {
                matches!(url.path(), "/" | "/entry" | "/config")
                    || url.path().starts_with("/frontend/evo/")
                    || url
                        .path()
                        .strip_prefix("/public/rng/dragon/servlet/CasinoGameServlet;jsession=")
                        .is_some_and(|session| {
                            !session.is_empty()
                                && session.bytes().all(|b| b.is_ascii_alphanumeric())
                        })
            }
            ("https", "affiliate0000001-static2.casinomodule.com") => {
                url.query().is_none()
                    && url.path().starts_with("/games/")
                    && url.path().ends_with(".js")
            }
            ("https", host) => host
                .strip_prefix("gserver-")
                .and_then(|rest| rest.strip_suffix(".redtiger.cash"))
                .is_some_and(|instance| {
                    instance.starts_with("evo-")
                        && instance
                            .bytes()
                            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
                        && url.query().is_none()
                        && url
                            .path()
                            .strip_prefix(&format!("/{instance}/platform/game/"))
                            .is_some_and(|suffix| matches!(suffix, "settings" | "spin"))
                }),
            _ => false,
        },
        "redtiger" => {
            url.scheme() == "https"
                && (matches!(
                    host,
                    "redtiger.com" | "jinx.prod.cmsevo.com" | "fansite.evo-games.com"
                ) || host.ends_with(".redtiger.cash"))
        }
        "playson" => matches!(
            host,
            "playson.com"
                | "xplatformwl-lrs.box-int-54f2g.com"
                | "games.box-int-54f2g.com"
                | "demo.0y81pm-c-o6pywcq.com"
        ),
        "pragmatic" => {
            host == "demogamesfree.pragmaticplay.net"
                || (host
                    .strip_prefix("demogamesfree-")
                    .and_then(|s| s.strip_suffix(".pragmaticplay.net"))
                    .is_some_and(|s| {
                        !s.is_empty()
                            && s.bytes()
                                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
                    })
                    && url.scheme() == "https"
                    && url.path().starts_with("/gs2c/common/"))
        }
        _ => false,
    };
    ensure!(
        allowed,
        "endpoint is outside the reviewed demo host allowlist for {provider}"
    );
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    async fn fixture(reply: &'static [u8]) -> (String, tokio::task::JoinHandle<Vec<u8>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut data = vec![0; 8192];
            let n = socket.read(&mut data).await.unwrap();
            data.truncate(n);
            socket.write_all(reply).await.unwrap();
            socket.shutdown().await.unwrap();
            data
        });
        (origin, task)
    }
    fn context(path: &std::path::Path, origin: &str) -> RunContext {
        let journal = Journal::create(&path.join("attempt"), &json!({})).unwrap();
        let mut ctx = RunContext::new("pragmatic", journal, Duration::from_secs(2), 20).unwrap();
        ctx.test_origin = Some(origin.into());
        ctx
    }
    #[tokio::test]
    async fn malformed_response_is_preserved_before_unknown_event() {
        let root = tempfile::tempdir().unwrap();
        let (origin, server) =
            fixture(b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\n{broken")
                .await;
        let ctx = context(root.path(), &origin);
        let error = ctx
            .json(
                "spin",
                &origin,
                &json!({"action":"spin"}),
                BTreeMap::new(),
                true,
            )
            .await
            .unwrap_err();
        assert!(error.downcast_ref::<crate::model::UnknownEvent>().is_some());
        assert!(
            String::from_utf8(server.await.unwrap())
                .unwrap()
                .contains("spin")
        );
        let journal = std::fs::read_to_string(ctx.journal.path().join("wire.jsonl")).unwrap();
        assert!(journal.contains(&STANDARD.encode(b"{broken")));
        assert!(journal.contains("http_request"));
    }
    #[tokio::test]
    async fn interrupted_body_keeps_received_prefix() {
        let root = tempfile::tempdir().unwrap();
        let (origin, server) =
            fixture(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\nprefix")
                .await;
        let ctx = context(root.path(), &origin);
        assert!(ctx.get("init", &origin).await.is_err());
        server.await.unwrap();
        let journal = std::fs::read_to_string(ctx.journal.path().join("wire.jsonl")).unwrap();
        assert!(journal.contains(&STANDARD.encode(b"prefix")));
        assert!(journal.contains("transport_error"));
    }
    #[tokio::test]
    async fn cookies_are_isolated_per_attempt() {
        let root = tempfile::tempdir().unwrap();
        let (origin, server) =
            fixture(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").await;
        let one = context(root.path(), &origin);
        one.seed_cookie("session=secret; Path=/", &origin).unwrap();
        let other = tempfile::tempdir().unwrap();
        let two = context(other.path(), &origin);
        assert!(two.cookies.cookies(&Url::parse(&origin).unwrap()).is_none());
        one.get("init", &origin).await.unwrap();
        let request = String::from_utf8(server.await.unwrap()).unwrap();
        assert!(request.contains("session=secret"));
        let journal = std::fs::read_to_string(one.journal.path().join("wire.jsonl")).unwrap();
        assert!(journal.contains(&STANDARD.encode("session=secret")));
    }
    #[test]
    fn unreviewed_targets_fail_without_network() {
        assert!(validate_target("pragmatic", "https://example.com/").is_err());
        assert!(validate_target("pragmatic", "http://demogamesfree.pragmaticplay.net/").is_err());
    }

    #[test]
    fn supplemental_demo_launchers_keep_host_and_path_boundaries() {
        assert!(
            validate_target(
                "irondog",
                "https://gamelauncherservice.desertorchidhub.com/"
            )
            .is_ok()
        );
        assert!(
            validate_target(
                "betsoft",
                "https://lobby-gp3demo.betsoftgaming.com/cwguestlogin.do?bankId=14425&gameId=1009"
            )
            .is_ok()
        );
        assert!(
            validate_target(
                "betsoft",
                "https://games-gp3demo.betsoftgaming.com/Venice_1.game"
            )
            .is_ok()
        );
        for target in [
            "https://games-gp3demo.betsoftgaming.com/folder/Venice.game",
            "https://games-gp3demo.betsoftgaming.com/Venice.game?redirect=1",
            "https://games-gp3demo.betsoftgaming.com/.game",
            "https://games.betsoftgaming.com/Venice.game",
        ] {
            assert!(validate_target("betsoft", target).is_err(), "{target}");
        }
    }
    #[tokio::test]
    async fn unexpected_websocket_greeting_and_binary_formats_are_alertable() {
        for kind in 0..3 {
            let root = tempfile::tempdir().unwrap();
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let origin = format!("ws://{}", listener.local_addr().unwrap());
            let server = tokio::spawn(async move {
                let (tcp, _) = listener.accept().await.unwrap();
                let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
                let frame = if kind == 0 {
                    Message::Text("unknown:greeting".into())
                } else {
                    Message::Binary(vec![0, 1, 2].into())
                };
                ws.send(frame).await.unwrap();
                tokio::time::sleep(Duration::from_millis(50)).await;
            });
            let ctx = context(root.path(), &origin);
            let mut socket = ctx
                .websocket(&origin, &origin, None, BTreeMap::new())
                .await
                .unwrap();
            let error = match kind {
                0 => ctx
                    .ws_expect_text(&mut socket, "ready", "1::")
                    .await
                    .unwrap_err(),
                1 => ctx
                    .ws_json(&mut socket, "spin", &json!({}), true, None)
                    .await
                    .unwrap_err(),
                _ => ctx
                    .ws_text_decoded(&mut socket, "spin", "{}", true, None, |s| {
                        Ok(serde_json::from_str(s)?)
                    })
                    .await
                    .unwrap_err(),
            };
            assert_eq!(
                crate::model::classify_error(&error),
                crate::model::CaptureStatus::UnknownEvent
            );
            let raw = std::fs::read_to_string(ctx.journal.path().join("wire.jsonl")).unwrap();
            assert!(raw.contains("ws_receive"));
            assert!(raw.contains(if kind == 0 {
                "unknown:greeting"
            } else {
                "AAEC"
            }));
            server.await.unwrap();
        }
    }
}
