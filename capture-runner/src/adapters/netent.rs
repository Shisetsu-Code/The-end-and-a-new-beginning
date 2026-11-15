//! Public NetEnt transport family dispatcher; contracts stay in native connectors.
use crate::{
    context::RunContext,
    model::{Completion, Game, Mode},
};
use anyhow::{Result, bail};
use serde_json::Value;
pub fn supports(game: &Game, mode: &Mode) -> bool {
    super::netent_geco::supports(game, mode)
        || super::netent_horizon::supports(game, mode)
        || super::netent_redtiger::supports(game, mode)
}
pub async fn execute(
    ctx: &RunContext,
    game: &Game,
    mode: &Mode,
    settings: &Value,
) -> Result<Completion> {
    if super::netent_redtiger::supports(game, mode) {
        return super::netent_redtiger::execute(ctx, game, mode, settings).await;
    }
    if super::netent_horizon::supports(game, mode) {
        return super::netent_horizon::execute(ctx, game, mode, settings).await;
    }
    if super::netent_geco::supports(game, mode) {
        return super::netent_geco::execute(ctx, game, mode, settings).await;
    }
    bail!("Netent game/mode has no native capture binding")
}
