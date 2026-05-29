mod support;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sdk = support::api_key_client()?;

    let current_profile = sdk.portfolio.get_current_profile().await?;
    println!("Current user ID: {}", current_profile.id);
    println!("Current account: {}", current_profile.account);
    if let Some(rank) = current_profile.rank {
        println!("Rank: {} (Fee: {} bps)", rank.name, rank.fee_rate_bps);
    }

    if let Some(profile_address) = support::optional_env("PROFILE_ADDRESS") {
        let profile = sdk.portfolio.get_profile(&profile_address).await?;
        println!("\nProfile by address ID: {}", profile.id);
        println!("Profile by address account: {}", profile.account);
    }

    let clob_positions = sdk.portfolio.get_clob_positions().await?;
    println!("\nCLOB Positions: {}", clob_positions.len());
    for position in clob_positions {
        println!("  {}", position.market.title);
        println!("    YES balance: {}", position.tokens_balance.yes);
        println!("    NO balance: {}", position.tokens_balance.no);
    }

    let amm_positions = sdk.portfolio.get_amm_positions().await?;
    println!("\nAMM Positions: {}", amm_positions.len());
    for position in amm_positions {
        println!(
            "  {} (outcome: {})",
            position.market.title, position.outcome_index
        );
        println!("    Collateral: {}", position.collateral_amount);
        println!("    PnL: {}", position.unrealized_pnl);
    }

    let history = sdk.portfolio.get_user_history(None, Some(10)).await?;
    println!("\nHistory: {} entries", history.data.len());
    for entry in &history.data {
        let slug = entry
            .market
            .as_ref()
            .map(|m| format!(" - {}", m.slug))
            .unwrap_or_default();
        println!(
            "  [{}] ts={}{}",
            entry.strategy.as_deref().unwrap_or("?"),
            entry.block_timestamp,
            slug
        );
    }
    if let Some(cursor) = &history.next_cursor {
        println!("  Next cursor: {}", cursor);
    }

    Ok(())
}
