use tower_sessions::SessionManagerLayer;
use tower_sessions_memory_store::MemoryStore;

pub type SessionStore = MemoryStore;

/// Create session manager layer
pub fn session_layer(_secret: &[u8]) -> SessionManagerLayer<MemoryStore> {
    let store = MemoryStore::default();
    SessionManagerLayer::new(store)
        .with_secure(false)
}
