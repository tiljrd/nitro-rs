use nitro_inbox::db::{Batch as DbBatch, Database as DbDatabase, Iterator as DbIterator};

pub struct SledDb {
    db: sled::Db,
}

impl SledDb {
    pub fn open(path: &str) -> anyhow::Result<Self> {
        let db = sled::open(path)?;
        Ok(Self { db })
    }
}

struct SledBatch {
    tree: sled::Db,
    ops: Vec<Op>,
}

enum Op {
    Put(Vec<u8>, Vec<u8>),
    Del(Vec<u8>),
}

impl SledBatch {
    fn new(tree: sled::Db) -> Self {
        Self { tree, ops: Vec::new() }
    }
}

impl DbBatch for SledBatch {
    fn put(&mut self, key: &[u8], value: &[u8]) -> Result<(), anyhow::Error> {
        self.ops.push(Op::Put(key.to_vec(), value.to_vec()));
        Ok(())
    }
    fn delete(&mut self, key: &[u8]) -> Result<(), anyhow::Error> {
        self.ops.push(Op::Del(key.to_vec()));
        Ok(())
    }
    fn write(self: Box<Self>) -> Result<(), anyhow::Error> {
        for op in self.ops {
            match op {
                Op::Put(k, v) => {
                    self.tree.insert(k, v)?;
                }
                Op::Del(k) => {
                    self.tree.remove(k)?;
                }
            }
        }
        self.tree.flush()?;
        Ok(())
    }
}

struct SledIter {
    iter: sled::Iter,
    current: Option<(Vec<u8>, Vec<u8>)>,
    error: Option<anyhow::Error>,
    prefix: Vec<u8>,
}

impl SledIter {
    fn new(tree: &sled::Db, prefix: &[u8], start: &[u8]) -> Self {
        let mut key = Vec::with_capacity(prefix.len() + start.len());
        key.extend_from_slice(prefix);
        key.extend_from_slice(start);
        let iter = tree.range(key..);
        Self { iter, current: None, error: None, prefix: prefix.to_vec() }
    }
}

impl DbIterator for SledIter {
    fn next(&mut self) -> bool {
        match self.iter.next() {
            Some(Ok((k, v))) => {
                if !k.starts_with(&self.prefix) {
                    self.current = None;
                    return false;
                }
                self.current = Some((k.to_vec(), v.to_vec()));
                true
            }
            Some(Err(e)) => {
                self.error = Some(anyhow::Error::from(e));
                self.current = None;
                false
            }
            None => {
                self.current = None;
                false
            }
        }
    }
    fn key(&self) -> &[u8] {
        if let Some((k, _)) = &self.current {
            k.as_slice()
        } else {
            &[]
        }
    }
    fn value(&self) -> &[u8] {
        if let Some((_, v)) = &self.current {
            v.as_slice()
        } else {
            &[]
        }
    }
    fn release(self: Box<Self>) {}
    fn error(&self) -> Option<anyhow::Error> {
        self.error.as_ref().map(|e| anyhow::anyhow!("{}", e))
    }
}

impl DbDatabase for SledDb {
    fn has(&self, key: &[u8]) -> Result<bool, anyhow::Error> {
        Ok(self.db.contains_key(key)?)
    }
    fn get(&self, key: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
        match self.db.get(key)? {
            Some(v) => Ok(v.to_vec()),
            None => Err(anyhow::anyhow!("not found")),
        }
    }
    fn put(&self, key: &[u8], value: &[u8]) -> Result<(), anyhow::Error> {
        self.db.insert(key, value.to_vec())?;
        Ok(())
    }
    fn new_batch(&self) -> Box<dyn DbBatch> {
        Box::new(SledBatch::new(self.db.clone()))
    }
    fn new_iterator(&self, prefix: &[u8], start: &[u8]) -> Box<dyn DbIterator> {
        Box::new(SledIter::new(&self.db, prefix, start))
    }
}
