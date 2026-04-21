mod support;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sdk = support::hmac_client()?;
    let tokens = sdk.api_tokens.list_tokens().await?;

    println!(
        "Active tokens visible to the scoped client: {}",
        tokens.len()
    );
    for token in &tokens {
        println!(
            "- {} label={:?} scopes={:?} lastUsedAt={:?}",
            token.token_id, token.label, token.scopes, token.last_used_at
        );
    }

    let Some(token_id) = support::optional_env("LIMITLESS_REVOKE_TOKEN_ID") else {
        println!(
            "\nSet LIMITLESS_REVOKE_TOKEN_ID to revoke a specific token. This example does not revoke anything by default."
        );
        return Ok(());
    };

    let response = sdk.api_tokens.revoke_token(&token_id).await?;
    println!("\nRevoke response: {}", response);

    Ok(())
}
