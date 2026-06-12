use std::collections::HashMap;

use crate::error::AuthError;
use crate::wire::parse_public_key_wire;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    Viewer,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct User {
    pub user_id: String,
    pub role: Role,
    pub public_key: String,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct UserFile {
    schema_version: u32,
    #[serde(default)]
    users: Vec<User>,
}

#[derive(Debug)]
pub struct UserStore {
    users: HashMap<String, User>,
}

impl UserStore {
    pub fn from_toml_str(s: &str) -> Result<Self, AuthError> {
        let file: UserFile =
            toml::from_str(s).map_err(|e| AuthError::UserStore(e.to_string()))?;
        if file.schema_version != 1 {
            return Err(AuthError::UserStore(format!(
                "unsupported schema_version {}",
                file.schema_version
            )));
        }
        let mut users = HashMap::new();
        for u in file.users {
            parse_public_key_wire(&u.public_key)
                .map_err(|e| AuthError::UserStore(format!("user `{}`: {e}", u.user_id)))?;
            if users.insert(u.user_id.clone(), u).is_some() {
                return Err(AuthError::UserStore("duplicate user_id".into()));
            }
        }
        Ok(Self { users })
    }

    pub fn get(&self, user_id: &str) -> Option<&User> {
        self.users.get(user_id)
    }

    pub fn len(&self) -> usize {
        self.users.len()
    }

    pub fn is_empty(&self) -> bool {
        self.users.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keypair::Keypair;

    fn toml_for(user_id: &str, key_wire: &str) -> String {
        format!(
            r#"
schema_version = 1

[[users]]
user_id = "{user_id}"
role = "admin"
public_key = "{key_wire}"
created_at = "2026-06-12"
note = "test user"
"#
        )
    }

    #[test]
    fn loads_valid_store() {
        let kp = Keypair::generate();
        let store = UserStore::from_toml_str(&toml_for("alice", &kp.public_wire())).unwrap();
        let user = store.get("alice").unwrap();
        assert_eq!(user.role, Role::Admin);
        assert_eq!(user.public_key, kp.public_wire());
        assert!(store.get("bob").is_none());
    }

    #[test]
    fn rejects_wrong_schema_version() {
        let err = UserStore::from_toml_str("schema_version = 2").unwrap_err();
        assert!(matches!(err, AuthError::UserStore(_)));
    }

    #[test]
    fn rejects_duplicate_user_id() {
        let kp = Keypair::generate();
        let one = toml_for("alice", &kp.public_wire());
        let dup = format!("{one}\n[[users]]\nuser_id = \"alice\"\nrole = \"viewer\"\npublic_key = \"{}\"\n", kp.public_wire());
        assert!(UserStore::from_toml_str(&dup).is_err());
    }

    #[test]
    fn rejects_unparseable_public_key() {
        let bad = toml_for("alice", "ed25519:not!!base64");
        assert!(UserStore::from_toml_str(&bad).is_err());
    }
}
