# Chapter 5.1: Schema Evolution

Schema evolution is the process of changing event and command structures over time while maintaining backward compatibility. EventCore provides powerful tools for handling schema changes gracefully.

## The Challenge

Your system evolves. Business requirements change. Data structures need to adapt. But in event sourcing, you can never change historical events - they're immutable facts about what happened.

```rust
// Day 1: Simple user registration
#[derive(Serialize, Deserialize)]
struct UserRegistered {
    user_id: UserId,
    email: String,
}

// 6 months later: Need more fields
#[derive(Serialize, Deserialize)]
struct UserRegistered {
    user_id: UserId,
    email: String,
    // New fields - but old events don't have them!
    first_name: String,
    last_name: String,
    preferences: UserPreferences,
}
```

## EventCore's Schema Evolution Approach

EventCore uses a combination of:
1. **Serde defaults** - Handle missing fields gracefully
2. **Event versioning** - Explicit version tracking
3. **Migration functions** - Transform old formats to new
4. **Schema registry** - Central type management

## Backward Compatible Changes

These changes don't break existing events:

### Adding Optional Fields

```rust
#[derive(Debug, Serialize, Deserialize)]
struct UserRegistered {
    user_id: UserId,
    email: String,
    
    // New optional fields with defaults
    #[serde(default)]
    first_name: Option<String>,
    
    #[serde(default)]
    last_name: Option<String>,
    
    #[serde(default)]
    preferences: UserPreferences,
}

impl Default for UserPreferences {
    fn default() -> Self {
        Self {
            newsletter: false,
            notifications: true,
            theme: Theme::Light,
        }
    }
}
```

### Adding Fields with Sensible Defaults

```rust
#[derive(Debug, Serialize, Deserialize)]
struct OrderPlaced {
    order_id: OrderId,
    customer_id: CustomerId,
    items: Vec<OrderItem>,
    
    // New field with computed default
    #[serde(default = "default_currency")]
    currency: Currency,
    
    // New field with timestamp default
    #[serde(default = "Utc::now")]
    placed_at: DateTime<Utc>,
}

fn default_currency() -> Currency {
    Currency::USD
}
```

### Adding Enum Variants

```rust
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
enum PaymentMethod {
    CreditCard { last_four: String },
    BankTransfer { account: String },
    PayPal { email: String },
    
    // New variants - old events still deserialize
    ApplePay { device_id: String },
    GooglePay { account_id: String },
    
    // Unknown variant fallback
    #[serde(other)]
    Unknown,
}
```

## Breaking Changes

These require explicit versioning:

### Removing Fields

```rust
// V1: Has deprecated field
#[derive(Debug, Serialize, Deserialize)]
struct UserRegisteredV1 {
    user_id: UserId,
    email: String,
    username: String, // Being removed
}

// V2: Field removed
#[derive(Debug, Serialize, Deserialize)]
struct UserRegisteredV2 {
    user_id: UserId,
    email: String,
    // username removed - breaking change!
}
```

### Changing Field Types

```rust
// V1: String user ID
#[derive(Debug, Serialize, Deserialize)]
struct UserRegisteredV1 {
    user_id: String, // String
    email: String,
}

// V2: Structured user ID
#[derive(Debug, Serialize, Deserialize)]
struct UserRegisteredV2 {
    user_id: UserId, // Custom type - breaking change!
    email: String,
}
```

### Restructuring Data

```rust
// V1: Flat structure
#[derive(Debug, Serialize, Deserialize)]
struct OrderPlacedV1 {
    order_id: OrderId,
    billing_street: String,
    billing_city: String,
    billing_state: String,
    shipping_street: String,
    shipping_city: String,
    shipping_state: String,
}

// V2: Nested structure
#[derive(Debug, Serialize, Deserialize)]
struct OrderPlacedV2 {
    order_id: OrderId,
    billing_address: Address,  // Restructured - breaking change!
    shipping_address: Address,
}
```

## Versioned Events

EventCore supports explicit event versioning:

```rust
use eventcore::serialization::VersionedEvent;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "version")]
enum UserRegisteredVersioned {
    #[serde(rename = "1")]
    V1 {
        user_id: String,
        email: String,
        username: String,
    },
    
    #[serde(rename = "2")]
    V2 {
        user_id: UserId,
        email: String,
        first_name: String,
        last_name: String,
    },
    
    #[serde(rename = "3")]
    V3 {
        user_id: UserId,
        email: String,
        profile: UserProfile, // Further evolution
    },
}

impl VersionedEvent for UserRegisteredVersioned {
    const EVENT_TYPE: &'static str = "UserRegistered";
    
    fn current_version() -> u32 {
        3
    }
    
    fn migrate_to_current(self) -> Self {
        match self {
            UserRegisteredVersioned::V1 { user_id, email, username } => {
                // V1 → V2: Convert string ID, extract names from username
                let (first_name, last_name) = split_username(&username);
                let user_id = UserId::try_new(user_id).unwrap_or_else(|_| UserId::new());
                
                UserRegisteredVersioned::V2 {
                    user_id,
                    email,
                    first_name,
                    last_name,
                }
            }
            UserRegisteredVersioned::V2 { user_id, email, first_name, last_name } => {
                // V2 → V3: Create profile from names
                UserRegisteredVersioned::V3 {
                    user_id,
                    email,
                    profile: UserProfile {
                        first_name,
                        last_name,
                        bio: None,
                        avatar_url: None,
                    },
                }
            }
            v3 => v3, // Already current version
        }
    }
}
```

## Migration Functions

For complex transformations, use migration functions:

```rust
use eventcore::serialization::{Migration, MigrationError};

struct UserRegisteredV1ToV2;

impl Migration<UserRegisteredV1, UserRegisteredV2> for UserRegisteredV1ToV2 {
    fn migrate(&self, v1: UserRegisteredV1) -> Result<UserRegisteredV2, MigrationError> {
        // Complex migration logic
        let user_id = parse_legacy_user_id(&v1.user_id)?;
        let (first_name, last_name) = extract_names_from_username(&v1.username)?;
        
        // Validate converted data
        if first_name.is_empty() {
            return Err(MigrationError::InvalidData("Empty first name".to_string()));
        }
        
        Ok(UserRegisteredV2 {
            user_id,
            email: v1.email,
            first_name,
            last_name,
        })
    }
}

fn parse_legacy_user_id(legacy_id: &str) -> Result<UserId, MigrationError> {
    // Handle legacy ID formats
    if legacy_id.starts_with("user_") {
        let numeric_part = legacy_id.strip_prefix("user_")
            .ok_or_else(|| MigrationError::InvalidData("Invalid legacy ID format".to_string()))?;
        
        let uuid = Uuid::new_v5(&Uuid::NAMESPACE_OID, numeric_part.as_bytes());
        Ok(UserId::from(uuid))
    } else if let Ok(uuid) = Uuid::parse_str(legacy_id) {
        Ok(UserId::from(uuid))
    } else {
        Err(MigrationError::InvalidData(format!("Cannot parse user ID: {}", legacy_id)))
    }
}
```

## Schema Registry

EventCore provides a schema registry for managing types:

```rust
use eventcore::serialization::{SchemaRegistry, TypeInfo};

#[derive(Default)]
struct MySchemaRegistry {
    registry: SchemaRegistry,
}

impl MySchemaRegistry {
    fn new() -> Self {
        let mut registry = SchemaRegistry::new();
        
        // Register event types with versions
        registry.register::<UserRegisteredV1>("UserRegistered", 1);
        registry.register::<UserRegisteredV2>("UserRegistered", 2);
        registry.register::<UserRegisteredV3>("UserRegistered", 3);
        
        // Register migrations
        registry.add_migration::<UserRegisteredV1, UserRegisteredV2>(
            UserRegisteredV1ToV2
        );
        registry.add_migration::<UserRegisteredV2, UserRegisteredV3>(
            UserRegisteredV2ToV3
        );
        
        Self { registry }
    }
    
    fn deserialize_event(&self, event_type: &str, version: u32, data: &[u8]) -> Result<Box<dyn Any>, SerializationError> {
        self.registry.deserialize_and_migrate(event_type, version, data)
    }
}
```

## Command Evolution

Commands evolve differently than events because they don't need historical compatibility:

```rust
// Commands can change more freely
#[derive(Command, Clone)]
struct CreateUser {
    // V1 fields
    email: Email,
    
    // V2 additions - no historical constraint
    first_name: FirstName,
    last_name: LastName,
    
    // V3 additions
    initial_preferences: UserPreferences,
    referral_code: Option<ReferralCode>,
}

// Use builder pattern for backward compatibility
impl CreateUser {
    pub fn builder() -> CreateUserBuilder {
        CreateUserBuilder::default()
    }
    
    // V1-style constructor
    pub fn from_email(email: Email) -> Self {
        Self {
            email,
            first_name: FirstName::default(),
            last_name: LastName::default(),
            initial_preferences: UserPreferences::default(),
            referral_code: None,
        }
    }
    
    // V2-style constructor
    pub fn with_name(email: Email, first_name: FirstName, last_name: LastName) -> Self {
        Self {
            email,
            first_name,
            last_name,
            initial_preferences: UserPreferences::default(),
            referral_code: None,
        }
    }
}

#[derive(Default)]
pub struct CreateUserBuilder {
    email: Option<Email>,
    first_name: Option<FirstName>,
    last_name: Option<LastName>,
    initial_preferences: Option<UserPreferences>,
    referral_code: Option<ReferralCode>,
}

impl CreateUserBuilder {
    pub fn email(mut self, email: Email) -> Self {
        self.email = Some(email);
        self
    }
    
    pub fn name(mut self, first: FirstName, last: LastName) -> Self {
        self.first_name = Some(first);
        self.last_name = Some(last);
        self
    }
    
    pub fn preferences(mut self, prefs: UserPreferences) -> Self {
        self.initial_preferences = Some(prefs);
        self
    }
    
    pub fn referral_code(mut self, code: ReferralCode) -> Self {
        self.referral_code = Some(code);
        self
    }
    
    pub fn build(self) -> Result<CreateUser, ValidationError> {
        Ok(CreateUser {
            email: self.email.ok_or(ValidationError::MissingField("email"))?,
            first_name: self.first_name.unwrap_or_default(),
            last_name: self.last_name.unwrap_or_default(),
            initial_preferences: self.initial_preferences.unwrap_or_default(),
            referral_code: self.referral_code,
        })
    }
}
```

## State Evolution

State structures also need to evolve with events:

```rust
#[derive(Default)]
struct UserState {
    exists: bool,
    email: String,
    
    // V2 fields with defaults
    first_name: Option<String>,
    last_name: Option<String>,
    
    // V3 fields
    profile: Option<UserProfile>,
    preferences: UserPreferences,
}

impl CommandLogic for CreateUser {
    type State = UserState;
    type Event = UserEvent;
    
    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
        match &event.payload {
            UserEvent::RegisteredV1 { user_id, email, username } => {
                state.exists = true;
                state.email = email.clone();
                // Legacy events don't have separate names
                state.first_name = None;
                state.last_name = None;
            }
            UserEvent::RegisteredV2 { user_id, email, first_name, last_name } => {
                state.exists = true;
                state.email = email.clone();
                state.first_name = Some(first_name.clone());
                state.last_name = Some(last_name.clone());
            }
            UserEvent::RegisteredV3 { user_id, email, profile } => {
                state.exists = true;
                state.email = email.clone();
                state.first_name = Some(profile.first_name.clone());
                state.last_name = Some(profile.last_name.clone());
                state.profile = Some(profile.clone());
            }
            // Handle other events...
        }
    }
}
```

## Projection Evolution

Projections need to handle schema changes too:

```rust
#[async_trait]
impl Projection for UserListProjection {
    type Event = UserEvent;
    type Error = ProjectionError;
    
    async fn apply(&mut self, event: &StoredEvent<Self::Event>) -> Result<(), Self::Error> {
        match &event.payload {
            // Handle all versions of user registration
            UserEvent::RegisteredV1 { user_id, email, username } => {
                let user = UserSummary {
                    id: user_id.clone(),
                    email: email.clone(),
                    display_name: username.clone(), // Use username as display name
                    first_name: None,
                    last_name: None,
                    created_at: event.occurred_at,
                };
                self.users.insert(user_id.clone(), user);
            }
            UserEvent::RegisteredV2 { user_id, email, first_name, last_name } => {
                let user = UserSummary {
                    id: user_id.clone(),
                    email: email.clone(),
                    display_name: format!("{} {}", first_name, last_name),
                    first_name: Some(first_name.clone()),
                    last_name: Some(last_name.clone()),
                    created_at: event.occurred_at,
                };
                self.users.insert(user_id.clone(), user);
            }
            UserEvent::RegisteredV3 { user_id, email, profile } => {
                let user = UserSummary {
                    id: user_id.clone(),
                    email: email.clone(),
                    display_name: profile.display_name(),
                    first_name: Some(profile.first_name.clone()),
                    last_name: Some(profile.last_name.clone()),
                    created_at: event.occurred_at,
                };
                self.users.insert(user_id.clone(), user);
            }
        }
        Ok(())
    }
}
```

## Migration Strategies

### Forward-Only Evolution

The simplest approach - only add fields, never remove:

```rust
#[derive(Debug, Serialize, Deserialize)]
struct ProductCreated {
    product_id: ProductId,
    name: String,
    price: Money,
    
    // V2 additions
    #[serde(default)]
    category: Option<Category>,
    #[serde(default)]
    tags: Vec<Tag>,
    
    // V3 additions
    #[serde(default)]
    metadata: ProductMetadata,
    #[serde(default)]
    variants: Vec<ProductVariant>,
    
    // V4 additions
    #[serde(default)]
    seo_info: Option<SeoInfo>,
    #[serde(default = "default_status")]
    status: ProductStatus,
}

fn default_status() -> ProductStatus {
    ProductStatus::Active
}
```

### Event Splitting

Split large events into focused ones:

```rust
// V1: Monolithic event
struct OrderProcessedV1 {
    order_id: OrderId,
    payment_method: PaymentMethod,
    payment_amount: Money,
    shipping_address: Address,
    items: Vec<OrderItem>,
    discount: Option<Discount>,
    tax_amount: Money,
}

// V2: Split into focused events
enum OrderEventV2 {
    PaymentProcessed {
        order_id: OrderId,
        payment_method: PaymentMethod,
        amount: Money,
    },
    ShippingAddressSet {
        order_id: OrderId,
        address: Address,
    },
    ItemsAdded {
        order_id: OrderId,
        items: Vec<OrderItem>,
    },
    DiscountApplied {
        order_id: OrderId,
        discount: Discount,
    },
    TaxCalculated {
        order_id: OrderId,
        amount: Money,
    },
}
```

### Lazy Migration

Migrate events only when needed:

```rust
use eventcore::serialization::LazyMigration;

#[derive(Clone)]
struct LazyUserEvent {
    raw_data: Vec<u8>,
    version: u32,
    migrated: Option<UserEvent>,
}

impl LazyUserEvent {
    fn get(&mut self) -> Result<&UserEvent, MigrationError> {
        if self.migrated.is_none() {
            let migrated = match self.version {
                1 => {
                    let v1: UserRegisteredV1 = serde_json::from_slice(&self.raw_data)?;
                    UserEvent::from_v1(v1)
                }
                2 => {
                    let v2: UserRegisteredV2 = serde_json::from_slice(&self.raw_data)?;
                    UserEvent::from_v2(v2)
                }
                3 => {
                    serde_json::from_slice(&self.raw_data)?
                }
                _ => return Err(MigrationError::UnsupportedVersion(self.version)),
            };
            self.migrated = Some(migrated);
        }
        Ok(self.migrated.as_ref().unwrap())
    }
}
```

## Testing Schema Evolution

### Migration Tests

```rust
#[cfg(test)]
mod migration_tests {
    use super::*;
    
    #[test]
    fn test_v1_to_v2_migration() {
        let v1_event = UserRegisteredV1 {
            user_id: "user_123".to_string(),
            email: "john.doe@example.com".to_string(),
            username: "john_doe".to_string(),
        };
        
        let migration = UserRegisteredV1ToV2;
        let v2_event = migration.migrate(v1_event).unwrap();
        
        assert!(v2_event.user_id.to_string().contains("123"));
        assert_eq!(v2_event.email, "john.doe@example.com");
        assert_eq!(v2_event.first_name, "john");
        assert_eq!(v2_event.last_name, "doe");
    }
    
    #[test]
    fn test_serialization_roundtrip() {
        let v2_event = UserRegisteredV2 {
            user_id: UserId::new(),
            email: "test@example.com".to_string(),
            first_name: "Test".to_string(),
            last_name: "User".to_string(),
        };
        
        // Serialize
        let json = serde_json::to_string(&v2_event).unwrap();
        
        // Deserialize
        let deserialized: UserRegisteredV2 = serde_json::from_str(&json).unwrap();
        
        assert_eq!(v2_event.user_id, deserialized.user_id);
        assert_eq!(v2_event.email, deserialized.email);
    }
    
    #[test]
    fn test_backward_compatibility() {
        // V1 JSON without new fields
        let v1_json = r#"{
            "user_id": "550e8400-e29b-41d4-a716-446655440000",
            "email": "legacy@example.com"
        }"#;
        
        // Should deserialize into V2 with defaults
        let v2_event: UserRegisteredV2 = serde_json::from_str(v1_json).unwrap();
        
        assert_eq!(v2_event.email, "legacy@example.com");
        assert!(v2_event.first_name.is_empty()); // Default
        assert!(v2_event.last_name.is_empty()); // Default
    }
}
```

### Property-Based Migration Tests

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn migration_preserves_core_data(
        user_id in any::<String>(),
        email in "[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\\.[a-zA-Z]{2,}",
        username in "[a-zA-Z0-9_]{3,20}",
    ) {
        let v1 = UserRegisteredV1 {
            user_id: user_id.clone(),
            email: email.clone(),
            username,
        };
        
        let migration = UserRegisteredV1ToV2;
        let v2 = migration.migrate(v1).unwrap();
        
        // Core data should be preserved
        prop_assert_eq!(v2.email, email);
        
        // User ID should be convertible
        prop_assert!(v2.user_id.to_string().len() > 0);
    }
}
```

## Best Practices

1. **Plan for evolution** - Design events with future changes in mind
2. **Use optional fields** - Default to optional for new fields
3. **Never remove fields** - Mark as deprecated instead
4. **Version breaking changes** - Use explicit versioning for major changes
5. **Test migrations thoroughly** - Especially edge cases
6. **Document schema changes** - Keep a changelog
7. **Migrate lazily** - Only when events are read
8. **Monitor migration performance** - Large migrations can be slow

## Summary

Schema evolution in EventCore:

- ✅ **Backward compatible** - Old events still work
- ✅ **Versioned explicitly** - Track breaking changes
- ✅ **Migration support** - Transform old formats
- ✅ **Type-safe** - Compile-time guarantees
- ✅ **Testable** - Comprehensive test support

Key patterns:
1. Use serde defaults for backward compatibility
2. Version events explicitly for breaking changes
3. Write migration functions for complex transformations
4. Test all migration paths thoroughly
5. Plan for evolution from day one

Next, let's explore [Event Versioning](./02-event-versioning.md) →