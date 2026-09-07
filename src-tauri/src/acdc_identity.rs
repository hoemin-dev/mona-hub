use serde_json::Value;

#[derive(serde::Serialize)]
pub struct Identity {
    pub tid: String,
    pub oid: String,
    pub name: String,
}

fn guid(value: &str) -> bool {
    value.len() == 36 && value.bytes().enumerate().all(|(i, b)| {
        if [8, 13, 18, 23].contains(&i) { b == b'-' } else { b.is_ascii_hexdigit() }
    })
}

// get-identity exposes configured custom claims in oidc_fields. Access id,
// user_uuid, sub and idp.id are NOT Entra object/tenant identifiers.
pub fn normalize(value: &Value, expected_tid: &str) -> Result<Identity, &'static str> {
    let fields = &value["oidc_fields"];
    log::info!("[AC/DC] schema tid_present={} oid_present={} name_present={} user_uuid_present={}",
        fields["tid"].is_string(), fields["oid"].is_string(),
        value["name"].is_string() || fields["name"].is_string(), value["user_uuid"].is_string());
    if value["service_token_status"].as_bool() == Some(true) || value["service_token_id"].as_str().is_some_and(|s| !s.is_empty()) {
        return Err("identity-invalid");
    }
    let tid = fields["tid"].as_str().ok_or("identity-claims-missing")?;
    let oid = fields["oid"].as_str().ok_or("identity-claims-missing")?;
    if !guid(tid) || !guid(oid) { return Err("identity-invalid"); }
    if !tid.eq_ignore_ascii_case(expected_tid) { return Err("tenant-mismatch"); }
    let name = value["name"].as_str().filter(|s| !s.trim().is_empty())
        .or_else(|| fields["name"].as_str()).unwrap_or("").trim();
    if name.len() > 1024 { return Err("identity-invalid"); }
    Ok(Identity { tid: tid.to_ascii_lowercase(), oid: oid.to_ascii_lowercase(), name: name.into() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    const TID: &str = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
    fn fixture() -> Value { json!({"name":"First name", "oidc_fields":{"tid":TID,"oid":"BBBBBBBB-BBBB-BBBB-BBBB-BBBBBBBBBBBB"}}) }
    #[test] fn accepts_custom_keys_and_top_level_name() {
        let result = normalize(&fixture(), TID).unwrap();
        assert_eq!(result.oid, "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb");
        assert_eq!(result.name, "First name");
    }
    #[test] fn name_is_optional_not_an_identity_key() {
        let mut v = fixture(); v.as_object_mut().unwrap().remove("name");
        assert_eq!(normalize(&v, TID).unwrap().name, "");
        v["oidc_fields"]["name"] = json!("Custom name");
        assert_eq!(normalize(&v, TID).unwrap().name, "Custom name");
    }
    #[test] fn rejects_missing_invalid_cross_tenant_and_service_identity() {
        assert!(normalize(&json!({"user_uuid":TID,"email":"fixture@example.test"}), TID).is_err());
        let mut v = fixture(); v["oidc_fields"]["oid"] = json!([TID]);
        assert!(normalize(&v, TID).is_err());
        assert!(normalize(&fixture(), "cccccccc-cccc-cccc-cccc-cccccccccccc").is_err());
        let mut v = fixture(); v["service_token_status"] = json!(true);
        assert!(normalize(&v, TID).is_err());
    }
}
