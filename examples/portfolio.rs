mod support;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sdk = support::api_key_client()?;
    let profile_address = support::require_env("PROFILE_ADDRESS");

    let profile = sdk.portfolio.get_profile(&profile_address).await?;
    println!("User ID: {}", profile.id);
    println!("Account: {}", profile.account);
    if let Some(rank) = profile.rank {
        println!("Rank: {} (Fee: {} bps)", rank.name, rank.fee_rate_bps);
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

    let history = sdk.portfolio.get_user_history(Some(1), Some(10)).await?;
    println!(
        "\nHistory: {} entries (total: {})",
        history.data.len(),
        history.total_count
    );
    for entry in history.data {
        print!("  [{}] {}", entry.entry_type, entry.created_at);
        if let Some(market_slug) = entry.market_slug {
            print!(" - {}", market_slug);
        }
        println!();
    }

    Ok(())
}
