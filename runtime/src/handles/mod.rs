//! Generational handles shared by runtime domains.
//!
//! A host value is not an object pointer.  It is an encoded [`HandleId`] with a
//! domain tag, slot, and generation.  Reusing a slot therefore cannot make an
//! old value silently refer to a new object.

use std::fmt;

pub const INVALID_HANDLE: i64 = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum HandleKind {
    List = 1,
    Map = 2,
    Range = 3,
    StringBuilder = 4,
    Tensor = 5,
    Async = 6,
    Api = 7,
    Database = 8,
    Duration = 9,
    Instant = 10,
    UtcDateTime = 11,
    MlModule = 12,
    MlDataset = 13,
    MlDataLoader = 14,
    MlDataFrame = 15,
    MlExperiment = 16,
    MlDistributedSession = 17,
    MlKvCache = 18,
    MlTokenizer = 19,
    MlVectorIndex = 20,
    MlArtifact = 21,
    AsyncScope = 22,
    AsyncCancel = 23,
    AsyncStream = 24,
    AsyncTcpListener = 25,
    AsyncTcpStream = 26,
    AsyncUdpSocket = 27,
    AsyncChannel = 28,
    ConcurrentTask = 29,
    ConcurrentBatch = 30,
    ConcurrentChannel = 31,
    ConcurrentCounter = 32,
    ServeServer = 33,
    ServeRequest = 34,
    ApiClientTimeout = 35,
    ApiClient = 70,
    ApiCorsPolicy = 36,
    ApiHandler = 37,
    ApiHandlerError = 38,
    ApiForm = 39,
    ApiFormSchema = 40,
    ApiFormBinding = 41,
    ApiTlsMode = 42,
    ApiServerEntry = 43,
    ApiRoute = 44,
    ApiRouter = 45,
    ApiRouteMatch = 46,
    ApiQuery = 47,
    ApiQuerySchema = 48,
    ApiQueryBinding = 49,
    ApiHttpRequest = 50,
    ApiHttpResponse = 51,
    ApiHttpHeader = 52,
    ApiHttpCookie = 53,
    ApiMultipart = 54,
    ApiMultipartPart = 55,
    ApiMiddlewareChain = 56,
    ApiMiddleware = 57,
    ApiMiddlewareTrace = 58,
    ApiMiddlewareLog = 71,
    ApiOAuthClient = 72,
    ApiOAuthToken = 73,
    ApiValidationSchema = 74,
    ApiValidationResult = 75,
    ApiError = 76,
    ApiCsrfPolicy = 77,
    ApiSsrfPolicy = 78,
    ApiSessionStore = 79,
    ApiSession = 80,
    ApiWebSocketServer = 81,
    ApiWebSocket = 82,
    ApiWebSocketMessage = 83,
    ApiWebSocketClient = 84,
    ApiSseServer = 85,
    ApiSseConnection = 86,
    ApiSseEvent = 87,
    ApiJsonValue = 88,
    DatabaseSqliteConnection = 59,
    DatabaseSqliteStatement = 60,
    DatabasePostgresConnection = 61,
    DatabasePostgresStatement = 62,
    DatabasePostgresChannel = 63,
    DatabasePostgresNotification = 64,
    DatabaseRedisConnection = 65,
    TracingConfig = 66,
    TracingSpan = 67,
    Set = 68,
    Iterator = 69,
    ApiHttp3Config = 89,
    ApiHttp3Handler = 90,
    ApiHttp3Server = 91,
    ApiHttp3Client = 92,
    ApiHttp3Request = 93,
    ApiHttp3Response = 94,
    ApiHttp3Outcome = 95,
    ApiGrpcMessage = 96,
    ApiGrpcClient = 97,
    ApiGrpcStream = 98,
    ApiGrpcMetadata = 99,
    ApiGrpcStatus = 100,
    ApiGrpcResponse = 101,
    ApiGrpcError = 102,
    ApiGrpcServer = 103,
    ApiGrpcService = 104,
    ApiGraphqlBuilder = 105,
    ApiGraphqlSchema = 106,
    ApiGraphqlSubscription = 107,
    ApiGraphqlResponse = 108,
    User = 255,
}

impl HandleKind {
    fn from_raw(value: u16) -> Option<Self> {
        Some(match value {
            1 => Self::List,
            2 => Self::Map,
            3 => Self::Range,
            4 => Self::StringBuilder,
            5 => Self::Tensor,
            6 => Self::Async,
            7 => Self::Api,
            8 => Self::Database,
            9 => Self::Duration,
            10 => Self::Instant,
            11 => Self::UtcDateTime,
            12 => Self::MlModule,
            13 => Self::MlDataset,
            14 => Self::MlDataLoader,
            15 => Self::MlDataFrame,
            16 => Self::MlExperiment,
            17 => Self::MlDistributedSession,
            18 => Self::MlKvCache,
            19 => Self::MlTokenizer,
            20 => Self::MlVectorIndex,
            21 => Self::MlArtifact,
            22 => Self::AsyncScope,
            23 => Self::AsyncCancel,
            24 => Self::AsyncStream,
            25 => Self::AsyncTcpListener,
            26 => Self::AsyncTcpStream,
            27 => Self::AsyncUdpSocket,
            28 => Self::AsyncChannel,
            29 => Self::ConcurrentTask,
            30 => Self::ConcurrentBatch,
            31 => Self::ConcurrentChannel,
            32 => Self::ConcurrentCounter,
            33 => Self::ServeServer,
            34 => Self::ServeRequest,
            35 => Self::ApiClientTimeout,
            70 => Self::ApiClient,
            36 => Self::ApiCorsPolicy,
            37 => Self::ApiHandler,
            38 => Self::ApiHandlerError,
            39 => Self::ApiForm,
            40 => Self::ApiFormSchema,
            41 => Self::ApiFormBinding,
            42 => Self::ApiTlsMode,
            43 => Self::ApiServerEntry,
            44 => Self::ApiRoute,
            45 => Self::ApiRouter,
            46 => Self::ApiRouteMatch,
            47 => Self::ApiQuery,
            48 => Self::ApiQuerySchema,
            49 => Self::ApiQueryBinding,
            50 => Self::ApiHttpRequest,
            51 => Self::ApiHttpResponse,
            52 => Self::ApiHttpHeader,
            53 => Self::ApiHttpCookie,
            54 => Self::ApiMultipart,
            55 => Self::ApiMultipartPart,
            56 => Self::ApiMiddlewareChain,
            57 => Self::ApiMiddleware,
            58 => Self::ApiMiddlewareTrace,
            71 => Self::ApiMiddlewareLog,
            72 => Self::ApiOAuthClient,
            73 => Self::ApiOAuthToken,
            74 => Self::ApiValidationSchema,
            75 => Self::ApiValidationResult,
            76 => Self::ApiError,
            77 => Self::ApiCsrfPolicy,
            68 => Self::Set,
            69 => Self::Iterator,
            89 => Self::ApiHttp3Config,
            90 => Self::ApiHttp3Handler,
            91 => Self::ApiHttp3Server,
            92 => Self::ApiHttp3Client,
            93 => Self::ApiHttp3Request,
            94 => Self::ApiHttp3Response,
            95 => Self::ApiHttp3Outcome,
            96 => Self::ApiGrpcMessage,
            97 => Self::ApiGrpcClient,
            98 => Self::ApiGrpcStream,
            99 => Self::ApiGrpcMetadata,
            100 => Self::ApiGrpcStatus,
            101 => Self::ApiGrpcResponse,
            102 => Self::ApiGrpcError,
            103 => Self::ApiGrpcServer,
            104 => Self::ApiGrpcService,
            105 => Self::ApiGraphqlBuilder,
            106 => Self::ApiGraphqlSchema,
            107 => Self::ApiGraphqlSubscription,
            108 => Self::ApiGraphqlResponse,
            255 => Self::User,
            81 => Self::ApiWebSocketServer,
            82 => Self::ApiWebSocket,
            83 => Self::ApiWebSocketMessage,
            84 => Self::ApiWebSocketClient,
            85 => Self::ApiSseServer,
            86 => Self::ApiSseConnection,
            87 => Self::ApiSseEvent,
            88 => Self::ApiJsonValue,
            59 => Self::DatabaseSqliteConnection,
            60 => Self::DatabaseSqliteStatement,
            61 => Self::DatabasePostgresConnection,
            62 => Self::DatabasePostgresStatement,
            63 => Self::DatabasePostgresChannel,
            64 => Self::DatabasePostgresNotification,
            65 => Self::DatabaseRedisConnection,
            66 => Self::TracingConfig,
            67 => Self::TracingSpan,
            68 => Self::Set,
            69 => Self::Iterator,
            255 => Self::User,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HandleId {
    kind: HandleKind,
    slot: u32,
    generation: u16,
}

impl HandleId {
    pub fn new(kind: HandleKind, slot: u32, generation: u16) -> Self {
        Self {
            kind,
            slot,
            generation: generation.max(1),
        }
    }

    pub fn kind(self) -> HandleKind {
        self.kind
    }

    pub fn slot(self) -> u32 {
        self.slot
    }

    pub fn generation(self) -> u16 {
        self.generation
    }

    pub fn raw(self) -> i64 {
        let value = ((self.kind as u64) << 48)
            | ((self.generation as u64) << 32)
            | (u64::from(self.slot) + 1);
        value as i64
    }

    pub fn from_raw(raw: i64) -> Result<Self, HandleError> {
        if raw <= 0 {
            return Err(HandleError::Invalid);
        }
        let value = raw as u64;
        let kind = HandleKind::from_raw((value >> 48) as u16).ok_or(HandleError::Invalid)?;
        let generation = ((value >> 32) & 0xFFFF) as u16;
        let encoded_slot = (value & 0xFFFF_FFFF) as u32;
        if generation == 0 || encoded_slot == 0 {
            return Err(HandleError::Invalid);
        }
        Ok(Self {
            kind,
            slot: encoded_slot - 1,
            generation,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandleError {
    Invalid,
    TypeMismatch {
        expected: HandleKind,
        actual: HandleKind,
    },
    Stale,
}

impl fmt::Display for HandleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid => write!(f, "invalid handle"),
            Self::TypeMismatch { expected, actual } => {
                write!(
                    f,
                    "handle type mismatch: expected {expected:?}, got {actual:?}"
                )
            }
            Self::Stale => write!(f, "stale or released handle"),
        }
    }
}

impl std::error::Error for HandleError {}

struct Slot<T> {
    generation: u16,
    value: Option<T>,
}

/// A typed, generational table for one runtime domain.
pub struct HandleTable<T> {
    kind: HandleKind,
    slots: Vec<Slot<T>>,
    free: Vec<u32>,
}

impl<T> HandleTable<T> {
    pub fn new(kind: HandleKind) -> Self {
        Self {
            kind,
            slots: Vec::new(),
            free: Vec::new(),
        }
    }

    pub fn insert(&mut self, value: T) -> HandleId {
        if let Some(slot) = self.free.pop() {
            let entry = &mut self.slots[slot as usize];
            entry.value = Some(value);
            return HandleId::new(self.kind, slot, entry.generation);
        }
        let slot = self.slots.len() as u32;
        self.slots.push(Slot {
            generation: 1,
            value: Some(value),
        });
        HandleId::new(self.kind, slot, 1)
    }

    /// Allocates a new slot even when released slots are available.
    ///
    /// This is used only by compatibility operations whose public ABI promises
    /// a contiguous range of handles (for example async task batches). Normal
    /// allocations should use [`Self::insert`] so released slots can be reused.
    pub fn insert_fresh(&mut self, value: T) -> HandleId {
        let slot = self.slots.len() as u32;
        self.slots.push(Slot {
            generation: 1,
            value: Some(value),
        });
        HandleId::new(self.kind, slot, 1)
    }

    fn validate_slot(&self, handle: HandleId) -> Result<usize, HandleError> {
        if handle.kind != self.kind {
            return Err(HandleError::TypeMismatch {
                expected: self.kind,
                actual: handle.kind,
            });
        }
        let slot = self
            .slots
            .get(handle.slot as usize)
            .ok_or(HandleError::Stale)?;
        if slot.generation != handle.generation {
            return Err(HandleError::Stale);
        }
        Ok(handle.slot as usize)
    }

    fn validate(&self, handle: HandleId) -> Result<usize, HandleError> {
        let index = self.validate_slot(handle)?;
        if self.slots[index].value.is_none() {
            return Err(HandleError::Stale);
        }
        Ok(index)
    }

    pub fn get(&self, handle: HandleId) -> Result<&T, HandleError> {
        let index = self.validate(handle)?;
        self.slots[index].value.as_ref().ok_or(HandleError::Stale)
    }

    pub fn get_mut(&mut self, handle: HandleId) -> Result<&mut T, HandleError> {
        let index = self.validate(handle)?;
        self.slots[index].value.as_mut().ok_or(HandleError::Stale)
    }

    /// Temporarily takes a value without releasing its handle. The caller must
    /// return it with [`Self::put`] before exposing the table again.
    pub fn take(&mut self, handle: HandleId) -> Result<T, HandleError> {
        let index = self.validate(handle)?;
        self.slots[index].value.take().ok_or(HandleError::Stale)
    }

    /// Returns a value previously removed with [`Self::take`] to the same
    /// generation and slot.
    pub fn put(&mut self, handle: HandleId, value: T) -> Result<(), HandleError> {
        let index = self.validate_slot(handle)?;
        if self.slots[index].value.is_some() {
            return Err(HandleError::Stale);
        }
        self.slots[index].value = Some(value);
        Ok(())
    }

    pub fn remove(&mut self, handle: HandleId) -> Result<T, HandleError> {
        let index = self.validate(handle)?;
        let slot = &mut self.slots[index];
        let value = slot.value.take().ok_or(HandleError::Stale)?;
        slot.generation = slot.generation.wrapping_add(1).max(1);
        self.free.push(handle.slot);
        Ok(value)
    }

    pub fn clear(&mut self) -> usize {
        self.drain().len()
    }

    /// Removes every live value and returns the handles that owned them.
    pub fn drain(&mut self) -> Vec<(HandleId, T)> {
        let mut removed = Vec::new();
        for (index, slot) in self.slots.iter_mut().enumerate() {
            if let Some(value) = slot.value.take() {
                let handle = HandleId::new(self.kind, index as u32, slot.generation);
                slot.generation = slot.generation.wrapping_add(1).max(1);
                self.free.push(index as u32);
                removed.push((handle, value));
            }
        }
        removed
    }

    pub fn len(&self) -> usize {
        self.slots
            .iter()
            .filter(|slot| slot.value.is_some())
            .count()
    }

    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn values(&self) -> impl Iterator<Item = &T> {
        self.slots.iter().filter_map(|slot| slot.value.as_ref())
    }

    pub fn iter(&self) -> impl Iterator<Item = (HandleId, &T)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(move |(index, slot)| {
                slot.value.as_ref().map(|value| {
                    (
                        HandleId::new(self.kind, index as u32, slot.generation),
                        value,
                    )
                })
            })
    }
}

impl<T> Default for HandleTable<T> {
    fn default() -> Self {
        Self::new(HandleKind::User)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn released_slot_cannot_alias_a_new_value() {
        let mut table = HandleTable::<&str>::new(HandleKind::List);
        let first = table.insert("first");
        assert_eq!(table.remove(first), Ok("first"));
        let second = table.insert("second");
        assert_ne!(first, second);
        assert_eq!(table.get(first), Err(HandleError::Stale));
        assert_eq!(table.get(second), Ok(&"second"));
    }

    #[test]
    fn kind_mismatch_is_rejected_before_slot_lookup() {
        let table = HandleTable::<()>::new(HandleKind::List);
        let handle = HandleTable::<()>::new(HandleKind::Map).insert(());
        assert_eq!(
            table.get(handle),
            Err(HandleError::TypeMismatch {
                expected: HandleKind::List,
                actual: HandleKind::Map
            })
        );
    }

    #[test]
    fn raw_zero_and_round_trip_are_explicit() {
        assert_eq!(
            HandleId::from_raw(INVALID_HANDLE),
            Err(HandleError::Invalid)
        );
        let id = HandleId::new(HandleKind::Tensor, 4, 9);
        assert_eq!(HandleId::from_raw(id.raw()), Ok(id));
    }

    #[test]
    fn database_handle_domains_round_trip_without_aliasing() {
        let kinds = [
            HandleKind::DatabaseSqliteConnection,
            HandleKind::DatabaseSqliteStatement,
            HandleKind::DatabasePostgresConnection,
            HandleKind::DatabasePostgresStatement,
            HandleKind::DatabasePostgresChannel,
            HandleKind::DatabasePostgresNotification,
            HandleKind::DatabaseRedisConnection,
            HandleKind::TracingConfig,
            HandleKind::TracingSpan,
        ];
        for kind in kinds {
            let handle = HandleId::new(kind, 3, 7);
            assert_eq!(HandleId::from_raw(handle.raw()), Ok(handle));
        }
    }
}
