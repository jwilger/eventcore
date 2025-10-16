# Data Encryption

Events are immutable and permanent. Encrypt sensitive data before storing it.

## Encryption Strategies

### Field-Level Encryption

Encrypt individual fields containing sensitive data:

```rust
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct EncryptedField {
    ciphertext: Vec<u8>,
    nonce: Vec<u8>,
    key_id: String, // For key rotation
}

impl EncryptedField {
    fn encrypt(
        plaintext: &str,
        key: &[u8; 32],
        key_id: String,
    ) -> Result<Self, EncryptionError> {
        let cipher = Aes256Gcm::new(key.into());
        let nonce = Nonce::from_slice(b"unique nonce"); // Use random nonce

        let ciphertext = cipher
            .encrypt(nonce, plaintext.as_bytes())
            .map_err(|_| EncryptionError::EncryptionFailed)?;

        Ok(Self {
            ciphertext,
            nonce: nonce.to_vec(),
            key_id,
        })
    }

    fn decrypt(&self, key: &[u8; 32]) -> Result<String, EncryptionError> {
        let cipher = Aes256Gcm::new(key.into());
        let nonce = Nonce::from_slice(&self.nonce);

        let plaintext = cipher
            .decrypt(nonce, self.ciphertext.as_ref())
            .map_err(|_| EncryptionError::DecryptionFailed)?;

        String::from_utf8(plaintext)
            .map_err(|_| EncryptionError::InvalidUtf8)
    }
}
```

### Event Payload Encryption

Encrypt entire event payloads:

```rust
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
enum SecureEvent {
    #[serde(rename = "encrypted")]
    Encrypted {
        payload: EncryptedField,
        event_type: String,
    },
    // Non-sensitive events can remain unencrypted
    SystemEvent(SystemEvent),
}

impl SecureEvent {
    fn encrypt_event<E: Serialize>(
        event: E,
        event_type: String,
        key: &[u8; 32],
        key_id: String,
    ) -> Result<Self, EncryptionError> {
        let json = serde_json::to_string(&event)?;
        let encrypted = EncryptedField::encrypt(&json, key, key_id)?;

        Ok(Self::Encrypted {
            payload: encrypted,
            event_type,
        })
    }
}
```

## Key Management

### Key Storage

Never store encryption keys in:

- Source code
- Configuration files
- Environment variables (in production)
- Event payloads

Use proper key management:

- AWS KMS
- Azure Key Vault
- HashiCorp Vault
- Hardware Security Modules (HSM)

### Key Rotation

Support key rotation without re-encrypting historical data:

```rust
struct KeyManager {
    current_key_id: String,
    keys: HashMap<String, Key>,
}

impl KeyManager {
    fn encrypt(&self, data: &str) -> Result<EncryptedField, Error> {
        let key = self.keys
            .get(&self.current_key_id)
            .ok_or(Error::KeyNotFound)?;

        EncryptedField::encrypt(data, &key.material, self.current_key_id.clone())
    }

    fn decrypt(&self, field: &EncryptedField) -> Result<String, Error> {
        // Use the key ID stored with the encrypted data
        let key = self.keys
            .get(&field.key_id)
            .ok_or(Error::KeyNotFound)?;

        field.decrypt(&key.material)
    }
}
```

## Encryption Patterns

### Deterministic Encryption

For fields that need to be searchable:

```rust
use sha2::{Sha256, Digest};

fn deterministic_encrypt(
    plaintext: &str,
    key: &[u8; 32],
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key);
    hasher.update(plaintext.as_bytes());

    base64::encode(hasher.finalize())
}

// Usage in events
#[derive(Serialize, Deserialize)]
struct UserRegistered {
    user_id: UserId,
    email_hash: String, // For lookups
    encrypted_email: EncryptedField, // Actual email
}
```

### Tokenization

Replace sensitive data with tokens:

```rust
#[derive(Debug, Clone)]
struct Token(String);

trait TokenVault {
    async fn tokenize(&self, value: &str) -> Result<Token, Error>;
    async fn detokenize(&self, token: &Token) -> Result<String, Error>;
}

// Store tokens in events instead of sensitive data
#[derive(Serialize, Deserialize)]
struct PaymentProcessed {
    payment_id: PaymentId,
    card_token: Token, // Not the actual card number
    amount: Money,
}
```

## Compliance Considerations

### GDPR - Right to Erasure

Since events are immutable, implement crypto-shredding:

```rust
impl KeyManager {
    async fn shred_user_data(&mut self, user_id: &UserId) -> Result<(), Error> {
        // Delete user-specific encryption keys
        self.user_keys.remove(user_id);

        // Events remain but are now unreadable
        Ok(())
    }
}
```

### PCI DSS

Never store in events:

- Full credit card numbers
- CVV/CVC codes
- PIN numbers
- Magnetic stripe data

### HIPAA

Encrypt all Protected Health Information (PHI):

- Patient names
- Medical record numbers
- Health conditions
- Treatment information

## Performance Considerations

1. **Batch Operations**: Encrypt/decrypt in batches when possible
2. **Caching**: Cache decrypted data with appropriate TTLs
3. **Async Operations**: Use async encryption for better throughput
4. **Hardware Acceleration**: Use AES-NI when available

## Example: Secure User Events

```rust
use eventcore::Event;

#[derive(Debug, Serialize, Deserialize)]
struct SecureUserEvent {
    #[serde(flatten)]
    base: Event,
    #[serde(flatten)]
    payload: SecureUserPayload,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
enum SecureUserPayload {
    UserRegistered {
        user_id: UserId,
        username: String, // Public
        email_hash: String, // For lookups
        encrypted_pii: EncryptedField, // Name, email, phone
    },
    ProfileUpdated {
        user_id: UserId,
        changes: Vec<ProfileChange>,
        encrypted_changes: Option<EncryptedField>,
    },
}

// Helper for building secure events
struct SecureEventBuilder<'a> {
    crypto: &'a CryptoService,
}

impl<'a> SecureEventBuilder<'a> {
    async fn user_registered(
        &self,
        user_id: UserId,
        username: String,
        email: String,
        pii: PersonalInfo,
    ) -> Result<SecureUserEvent, Error> {
        let email_hash = self.crypto.hash_email(&email);
        let encrypted_pii = self.crypto.encrypt_pii(&pii).await?;

        Ok(SecureUserEvent {
            base: Event::new(),
            payload: SecureUserPayload::UserRegistered {
                user_id,
                username,
                email_hash,
                encrypted_pii,
            },
        })
    }
}
```
