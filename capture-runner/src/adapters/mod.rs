//! One registry drives both planning and execution; no legacy runner dependency.
use crate::{
    context::RunContext,
    model::{Completion, Game, Mode},
};
use anyhow::Result;
use serde_json::Value;
pub fn unsupported_reason(provider: &str, game: &Game, mode: &Mode) -> &'static str {
    if provider == "habanero"
        && let Some(detail) = habanero::missing_binding_detail(game, mode)
    {
        return detail;
    }
    "native selector or continuation binding is not included for this game/mode"
}
pub mod amigogaming;
pub mod amusnet;
pub mod belatra;
mod belatra_crypto;
pub mod betsoft_live;
pub mod bgaming;
pub mod egt_digital;
pub mod ela_games;
pub mod elk;
pub mod endorphina;
pub mod evoplay;
pub mod gamesglobal;
pub mod habanero;
pub mod hacksaw;
pub mod irondog;
pub mod kagaming;
pub mod kalamba;
pub mod kendoo;
pub mod mascot;
pub mod microgaming;
pub mod netent;
mod netent_geco;
mod netent_horizon;
mod netent_launch;
mod netent_redtiger;
pub mod nolimit;
pub mod onespin4win;
pub mod peterandsons;
pub mod playngo;
pub mod playson;
pub mod playtech;
pub mod pragmatic;
pub mod redtiger;
pub mod rubyplay;
pub mod smartsoft;
pub mod spinomenal;
pub mod threeoaks;
pub mod thunderspin;
pub mod yggdrasil;
pub fn supports(provider: &str, game: &Game, mode: &Mode) -> bool {
    match provider {
        "1spin4win" => onespin4win::supports(game, mode),
        "amigogaming" => amigogaming::supports(game, mode),
        "amusnet" => amusnet::supports(game, mode),
        "belatra" => belatra::supports(game, mode),
        "betsoft" => betsoft_live::supports(game, mode),
        "bgaming" => bgaming::supports(game, mode),
        "egt-digital" => egt_digital::supports(game, mode),
        "ela-games" => ela_games::supports(game, mode),
        "elk" => elk::supports(game, mode),
        "endorphina" => endorphina::supports(game, mode),
        "evoplay" => evoplay::supports(game, mode),
        "gamesglobal" => gamesglobal::supports(game, mode),
        "habanero" => habanero::supports(game, mode),
        "hacksaw" => hacksaw::supports(game, mode),
        "hardfire-kalamba" => kalamba::supports(game, mode),
        "irondog" => irondog::supports(game, mode),
        "kagaming" => kagaming::supports(game, mode),
        "kendoo" => kendoo::supports(game, mode),
        "mascot" => mascot::supports(game, mode),
        "microgaming" => microgaming::supports(game, mode),
        "netent" => netent::supports(game, mode),
        "nolimit" => nolimit::supports(game, mode),
        "peterandsons" => peterandsons::supports(game, mode),
        "playngo" => playngo::supports(game, mode),
        "playson" => playson::supports(game, mode),
        "playtech" => playtech::supports(game, mode),
        "pragmatic" => pragmatic::supports(game, mode),
        "redtiger" => redtiger::supports(game, mode),
        "reelkingdom" => pragmatic::supports(game, mode),
        "rubyplay" => rubyplay::supports(game, mode),
        "smartsoft" => smartsoft::supports(game, mode),
        "spinomenal" => spinomenal::supports(game, mode),
        "threeoaks" => threeoaks::supports(game, mode),
        "thunderspin" => thunderspin::supports(game, mode),
        "yggdrasil" => yggdrasil::supports(game, mode),
        _ => false,
    }
}
pub async fn execute(
    ctx: &RunContext,
    provider: &str,
    game: &Game,
    mode: &Mode,
    settings: &Value,
) -> Result<Completion> {
    match provider {
        "1spin4win" => onespin4win::execute(ctx, game, mode, settings).await,
        "amigogaming" => amigogaming::execute(ctx, game, mode, settings).await,
        "amusnet" => amusnet::execute(ctx, game, mode, settings).await,
        "belatra" => belatra::execute(ctx, game, mode, settings).await,
        "betsoft" => betsoft_live::execute(ctx, game, mode, settings).await,
        "bgaming" => bgaming::execute(ctx, game, mode, settings).await,
        "egt-digital" => egt_digital::execute(ctx, game, mode, settings).await,
        "ela-games" => ela_games::execute(ctx, game, mode, settings).await,
        "elk" => elk::execute(ctx, game, mode, settings).await,
        "endorphina" => endorphina::execute(ctx, game, mode, settings).await,
        "evoplay" => evoplay::execute(ctx, game, mode, settings).await,
        "gamesglobal" => gamesglobal::execute(ctx, game, mode, settings).await,
        "habanero" => habanero::execute(ctx, game, mode, settings).await,
        "hacksaw" => hacksaw::execute(ctx, game, mode, settings).await,
        "hardfire-kalamba" => kalamba::execute(ctx, game, mode, settings).await,
        "irondog" => irondog::execute(ctx, game, mode, settings).await,
        "kagaming" => kagaming::execute(ctx, game, mode, settings).await,
        "kendoo" => kendoo::execute(ctx, game, mode, settings).await,
        "mascot" => mascot::execute(ctx, game, mode, settings).await,
        "microgaming" => microgaming::execute(ctx, game, mode, settings).await,
        "netent" => netent::execute(ctx, game, mode, settings).await,
        "nolimit" => nolimit::execute(ctx, game, mode, settings).await,
        "peterandsons" => peterandsons::execute(ctx, game, mode, settings).await,
        "playngo" => playngo::execute(ctx, game, mode, settings).await,
        "playson" => playson::execute(ctx, game, mode, settings).await,
        "playtech" => playtech::execute(ctx, game, mode, settings).await,
        "pragmatic" => pragmatic::execute(ctx, game, mode, settings).await,
        "redtiger" => redtiger::execute(ctx, game, mode, settings).await,
        "reelkingdom" => pragmatic::execute(ctx, game, mode, settings).await,
        "rubyplay" => rubyplay::execute(ctx, game, mode, settings).await,
        "smartsoft" => smartsoft::execute(ctx, game, mode, settings).await,
        "spinomenal" => spinomenal::execute(ctx, game, mode, settings).await,
        "threeoaks" => threeoaks::execute(ctx, game, mode, settings).await,
        "thunderspin" => thunderspin::execute(ctx, game, mode, settings).await,
        "yggdrasil" => yggdrasil::execute(ctx, game, mode, settings).await,
        _ => anyhow::bail!("unsupported provider {provider}"),
    }
}
