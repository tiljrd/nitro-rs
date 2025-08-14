use async_trait::async_trait;

pub trait Batch {
    fn put(&mut self, key: &[u8], value: &[u8]) -> Result<(), anyhow::Error>;
    fn delete(&mut self, key: &[u8]) -> Result<(), anyhow::Error>;
    fn write(self: Box<Self>) -> Result<(), anyhow::Error>;
}

pub trait Iterator {
    fn next(&mut self) -> bool;
    fn key(&self) -> &[u8];
    fn release(self: Box<Self>);
    fn error(&self) -> Option<anyhow::Error>;
}

pub trait Database: Send + Sync {
    fn has(&self, key: &[u8]) -> Result<bool, anyhow::Error>;
    fn get(&self, key: &[u8]) -> Result<Vec<u8>, anyhow::Error>;
    fn put(&self, key: &[u8], value: &[u8]) -> Result<(), anyhow::Error>;
    fn new_batch(&self) -> Box<dyn Batch>;
    fn new_iterator(&self, prefix: &[u8], start: &[u8]) -> Box<dyn Iterator>;
}
