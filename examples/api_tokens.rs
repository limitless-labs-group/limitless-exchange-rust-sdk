mod support;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sdk = support::hmac_client()?;
    let tokens = sdk.api_tokens.list_tokens().await?;

    println!("Active tokens: {}", tokens.len());
    for token in tokens {
        println!(
            "- {} scopes={:?} createdAt={}",
            token.token_id, token.scopes, token.created_at
        );
    }

    let Some(identity_token) = support::optional_env("LIMITLESS_IDENTITY_TOKEN") else {
        println!("\nSet LIMITLESS_IDENTITY_TOKEN to also fetch partner capabilities.");
        return Ok(());
    };

    let capabilities = sdk.api_tokens.get_capabilities(&identity_token).await?;
    println!("\nPartner profile: {}", capabilities.partner_profile_id);
    println!(
        "Token management enabled: {}",
        capabilities.token_management_enabled
    );
    println!("Allowed scopes: {:?}", capabilities.allowed_scopes);

    Ok(())
}
