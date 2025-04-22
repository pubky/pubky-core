use pkarr::PublicKey;


/// Custom validator for the zbase32 pubkey in the route path.
/// Usage:
/// ```rust
/// pub async fn my_handler(
///     Path(pubkey): Path<Z32Pubkey>,
/// ) -> HttpResult<impl IntoResponse> {
///     println!("Pubkey: {}", pubkey.0);
///     Ok((StatusCode::OK, "Ok"))
/// }
/// ```
/// 
/// 
#[derive(Debug)]
pub(crate)struct Z32Pubkey(pub PublicKey);

impl<'de> serde::Deserialize<'de> for Z32Pubkey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s: String = serde::Deserialize::deserialize(deserializer)?;
        let pubkey = PublicKey::try_from(s.as_str())
            .map_err(serde::de::Error::custom)?;
        Ok(Z32Pubkey(pubkey))
    }
}