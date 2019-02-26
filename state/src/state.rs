use super::account::StateObject;
use super::errors::Error;
use super::object_entry::{ObjectStatus, StateObjectEntry};
use cita_trie::codec::RLPNodeCodec;
use cita_trie::db::DB;
use cita_trie::trie::PatriciaTrie;
use cita_trie::trie::Trie;
use ethereum_types::{Address, H256, U256};
use std::cell::RefCell;
use std::collections::hash_map::Entry;
use std::collections::HashMap;

pub struct State<B> {
    pub db: B,
    pub root: H256,
    pub cache: RefCell<HashMap<Address, StateObjectEntry>>,
    pub checkpoints: RefCell<Vec<HashMap<Address, Option<StateObjectEntry>>>>,
}

impl<B: DB> State<B> {
    /// Creates empty state for test.
    pub fn new(mut db: B) -> Result<State<B>, Error> {
        let mut trie = PatriciaTrie::new(&mut db, RLPNodeCodec::default());
        let root = trie.root()?;

        Ok(State {
            db,
            root: From::from(&root[..]),
            cache: RefCell::new(HashMap::new()),
            checkpoints: RefCell::new(Vec::new()),
        })
    }

    /// Creates new state with existing state root
    pub fn from_existing(db: B, root: H256) -> Result<State<B>, Error> {
        if !db
            .contains(&root.0[..])
            .or_else(|e| Err(Error::DB(format!("{}", e))))?
        {
            return Err(Error::NotFound);
        }
        Ok(State {
            db,
            root,
            cache: RefCell::new(HashMap::new()),
            checkpoints: RefCell::new(Vec::new()),
        })
    }

    /// Create a contract account with code or not
    /// Overwrite the code if the contract already exists
    pub fn new_contract(
        &mut self,
        contract: &Address,
        balance: U256,
        nonce: U256,
        code: Option<Vec<u8>>,
    ) -> StateObject {
        let mut state_object = StateObject::new(balance, nonce);
        state_object.init_code(code.unwrap_or_default());

        self.insert_cache(
            contract,
            StateObjectEntry::new_dirty(Some(state_object.clone_dirty())),
        );
        state_object
    }

    pub fn kill_contract(&mut self, contract: &Address) {
        self.insert_cache(contract, StateObjectEntry::new_dirty(None));
    }

    /// Clear cache
    /// Note that the cache is just a HashMap, so memory explosion will be
    /// happend if you never call `clear()`. You should decide for yourself
    /// when to call this function.
    pub fn clear(&mut self) {
        assert!(self.checkpoints.borrow().is_empty());
        self.cache.borrow_mut().clear();
    }

    /// Get state object
    /// Firstly, search from cache. If not, get from trie.
    pub fn get_state_object(&mut self, address: &Address) -> Result<Option<StateObject>, Error> {
        if let Some(state_object_entry) = self.cache.borrow().get(address) {
            if let Some(state_object) = &state_object_entry.state_object {
                return Ok(Some((*state_object).clone_dirty()));
            }
        }
        let trie = PatriciaTrie::from(&mut self.db, RLPNodeCodec::default(), &self.root.0)?;
        match trie.get(&address)? {
            Some(rlp) => {
                let mut state_object = StateObject::from_rlp(&rlp)?;
                state_object.read_code(&mut self.db)?;
                self.insert_cache(
                    address,
                    StateObjectEntry::new_clean(Some(state_object.clone_clean())),
                );
                Ok(Some(state_object))
            }
            None => Ok(None),
        }
    }

    /// Get state object from cache or trie.
    /// If not exist, create one and then insert into cache.
    pub fn get_state_object_or_default(&mut self, address: &Address) -> Result<StateObject, Error> {
        match self.get_state_object(address)? {
            Some(state_object) => Ok(state_object),
            None => {
                let state_object = self.new_contract(address, U256::zero(), U256::zero(), None);
                Ok(state_object)
            }
        }
    }

    pub fn exist(&mut self, a: &Address) -> Result<bool, Error> {
        Ok(self.get_state_object(a)?.is_some())
    }

    pub fn set_storage(&mut self, address: &Address, key: H256, value: H256) -> Result<(), Error> {
        let mut state_object = self.get_state_object_or_default(address)?;
        if state_object.get_storage(&mut self.db, &key)? == Some(value) {
            return Ok(());
        }

        self.add_checkpoint(address);
        if let Some(ref mut state_object_entry) = self.cache.borrow_mut().get_mut(address) {
            match state_object_entry.state_object {
                Some(ref mut state_object) => {
                    state_object.set_storage(key, value);
                    state_object_entry.status = ObjectStatus::Dirty;
                }
                None => panic!("state object always exist in cache."),
            }
        }
        Ok(())
    }

    fn insert_cache(&self, address: &Address, state_object_entry: StateObjectEntry) {
        let is_dirty = state_object_entry.is_dirty();
        let old_entry = self
            .cache
            .borrow_mut()
            .insert(*address, state_object_entry.clone_dirty());

        if is_dirty {
            if let Some(checkpoint) = self.checkpoints.borrow_mut().last_mut() {
                checkpoint.entry(*address).or_insert(old_entry);
            }
        }
    }

    pub fn commit(&mut self) -> Result<(), Error> {
        assert!(self.checkpoints.borrow().is_empty());

        // firstly, update account storage tree
        for (_address, entry) in self
            .cache
            .borrow_mut()
            .iter_mut()
            .filter(|&(_, ref a)| a.is_dirty())
        {
            if let Some(ref mut state_object) = entry.state_object {
                state_object.commit_storage(&mut self.db)?;
                state_object.commit_code(&mut self.db)?;
            }
        }

        // secondly, update the world state tree
        let mut trie = PatriciaTrie::from(&mut self.db, RLPNodeCodec::default(), &self.root.0)?;
        for (address, entry) in self
            .cache
            .borrow_mut()
            .iter_mut()
            .filter(|&(_, ref a)| a.is_dirty())
        {
            entry.status = ObjectStatus::Committed;
            match entry.state_object {
                Some(ref mut state_object) => {
                    trie.insert(address, &rlp::encode(&state_object.account()))?;
                }
                None => {
                    trie.remove(address)?;
                }
            }
        }
        self.root = From::from(&trie.root()?[..]);
        Ok(())
    }

    /// Create a recoverable checkpoint of this state. Return the checkpoint index.
    pub fn checkpoint(&mut self) -> usize {
        let mut checkpoints = self.checkpoints.borrow_mut();
        let index = checkpoints.len();
        checkpoints.push(HashMap::new());
        index
    }

    fn add_checkpoint(&self, address: &Address) {
        if let Some(ref mut checkpoint) = self.checkpoints.borrow_mut().last_mut() {
            checkpoint.entry(*address).or_insert_with(|| {
                self.cache
                    .borrow()
                    .get(address)
                    .map(StateObjectEntry::clone_dirty)
            });
        }
    }

    /// Merge last checkpoint with previous.
    pub fn discard_checkpoint(&mut self) {
        let last = self.checkpoints.borrow_mut().pop();
        if let Some(mut checkpoint) = last {
            if let Some(prev) = self.checkpoints.borrow_mut().last_mut() {
                if prev.is_empty() {
                    *prev = checkpoint;
                } else {
                    for (k, v) in checkpoint.drain() {
                        prev.entry(k).or_insert(v);
                    }
                }
            }
        }
    }

    /// Revert to the last checkpoint and discard it.
    pub fn revert_checkpoint(&mut self) {
        if let Some(mut last) = self.checkpoints.borrow_mut().pop() {
            for (k, v) in last.drain() {
                match v {
                    Some(v) => match self.cache.get_mut().entry(k) {
                        Entry::Occupied(mut e) => {
                            // Merge checkpointed changes back into the main account
                            // storage preserving the cache.
                            e.get_mut().merge(v);
                        }
                        Entry::Vacant(e) => {
                            e.insert(v);
                        }
                    },
                    None => {
                        if let Entry::Occupied(e) = self.cache.get_mut().entry(k) {
                            if e.get().is_dirty() {
                                e.remove();
                            }
                        }
                    }
                }
            }
        }
    }
}

pub trait StateObjectInfo {
    fn nonce(&mut self, a: &Address) -> Result<U256, Error>;

    fn balance(&mut self, a: &Address) -> Result<U256, Error>;

    fn get_storage(&mut self, a: &Address, key: &H256) -> Result<H256, Error>;

    fn code(&mut self, a: &Address) -> Result<Vec<u8>, Error>;

    fn set_code(&mut self, a: &Address, code: Vec<u8>) -> Result<(), Error>;

    fn code_hash(&mut self, a: &Address) -> Result<H256, Error>;

    fn code_size(&mut self, a: &Address) -> Result<usize, Error>;

    fn add_balance(&mut self, a: &Address, incr: U256) -> Result<(), Error>;

    fn sub_balance(&mut self, a: &Address, decr: U256) -> Result<(), Error>;

    fn transfer_balance(&mut self, from: &Address, to: &Address, by: U256) -> Result<(), Error>;

    fn inc_nonce(&mut self, a: &Address) -> Result<(), Error>;
}

impl<B: DB> StateObjectInfo for State<B> {
    fn nonce(&mut self, a: &Address) -> Result<U256, Error> {
        Ok(self
            .get_state_object(a)?
            .map_or(U256::zero(), |e| e.nonce()))
    }

    fn balance(&mut self, a: &Address) -> Result<U256, Error> {
        Ok(self
            .get_state_object(a)?
            .map_or(U256::zero(), |e| e.balance()))
    }

    fn get_storage(&mut self, a: &Address, key: &H256) -> Result<H256, Error> {
        match self.get_state_object(a)? {
            Some(mut state_object) => match state_object.get_storage(&mut self.db, key)? {
                Some(v) => Ok(v),
                None => Ok(H256::zero()),
            },
            None => Ok(H256::zero()),
        }
    }

    fn code(&mut self, a: &Address) -> Result<Vec<u8>, Error> {
        Ok(self.get_state_object(a)?.map_or(vec![], |e| e.code()))
    }

    fn set_code(&mut self, a: &Address, code: Vec<u8>) -> Result<(), Error> {
        let mut state_object = self.get_state_object_or_default(a)?;
        state_object.init_code(code);
        self.insert_cache(a, StateObjectEntry::new_dirty(Some(state_object)));
        Ok(())
    }

    fn code_hash(&mut self, a: &Address) -> Result<H256, Error> {
        Ok(self
            .get_state_object(a)?
            .map_or(H256::zero(), |e| e.code_hash()))
    }

    fn code_size(&mut self, a: &Address) -> Result<usize, Error> {
        Ok(self.get_state_object(a)?.map_or(0, |e| e.code_size()))
    }

    fn add_balance(&mut self, a: &Address, incr: U256) -> Result<(), Error> {
        if incr.is_zero() {
            return Ok(());
        }
        let mut state_object = self.get_state_object_or_default(a)?;
        state_object.add_balance(incr);
        self.insert_cache(a, StateObjectEntry::new_dirty(Some(state_object)));
        Ok(())
    }

    fn sub_balance(&mut self, a: &Address, decr: U256) -> Result<(), Error> {
        if decr.is_zero() {
            return Ok(());
        }
        let mut state_object = self.get_state_object_or_default(a)?;
        state_object.sub_balance(decr);
        self.insert_cache(a, StateObjectEntry::new_dirty(Some(state_object)));
        Ok(())
    }

    fn transfer_balance(&mut self, from: &Address, to: &Address, by: U256) -> Result<(), Error> {
        self.sub_balance(from, by)?;
        self.add_balance(to, by)?;
        Ok(())
    }

    fn inc_nonce(&mut self, a: &Address) -> Result<(), Error> {
        let mut state_object = self.get_state_object_or_default(a)?;
        state_object.inc_nonce();
        self.insert_cache(a, StateObjectEntry::new_dirty(Some(state_object)));
        Ok(())
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use cita_trie::db::MemoryDB;

    fn get_temp_state() -> State<MemoryDB> {
        let db = MemoryDB::new();
        State::new(db).unwrap()
    }

    #[test]
    fn test_code_from_database() {
        let a = Address::zero();
        let (root, db) = {
            let mut state = get_temp_state();
            state.set_code(&a, vec![1, 2, 3]).unwrap();
            assert_eq!(state.code(&a).unwrap(), vec![1, 2, 3]);
            assert_eq!(
                state.code_hash(&a).unwrap(),
                "0xfd1780a6fc9ee0dab26ceb4b3941ab03e66ccd970d1db91612c66df4515b0a0a".into()
            );
            assert_eq!(state.code_size(&a).unwrap(), 3);
            state.commit().unwrap();
            assert_eq!(state.code(&a).unwrap(), vec![1, 2, 3]);
            assert_eq!(
                state.code_hash(&a).unwrap(),
                "0xfd1780a6fc9ee0dab26ceb4b3941ab03e66ccd970d1db91612c66df4515b0a0a".into()
            );
            assert_eq!(state.code_size(&a).unwrap(), 3);
            (state.root, state.db)
        };

        let mut state = State::from_existing(db, root).unwrap();
        assert_eq!(state.code(&a).unwrap(), vec![1, 2, 3]);
        assert_eq!(
            state.code_hash(&a).unwrap(),
            "0xfd1780a6fc9ee0dab26ceb4b3941ab03e66ccd970d1db91612c66df4515b0a0a".into()
        );
        assert_eq!(state.code_size(&a).unwrap(), 3);
    }

    #[test]
    fn get_storage_from_datebase() {
        let a = Address::zero();
        let (root, db) = {
            let mut state = get_temp_state();
            state
                .set_storage(
                    &a,
                    H256::from(&U256::from(1u64)),
                    H256::from(&U256::from(69u64)),
                )
                .unwrap();
            state.commit().unwrap();
            (state.root, state.db)
        };

        let mut state = State::from_existing(db, root).unwrap();
        assert_eq!(
            state
                .get_storage(&a, &H256::from(&U256::from(1u64)))
                .unwrap(),
            H256::from(&U256::from(69u64))
        );
    }

    #[test]
    fn get_from_database() {
        let a = Address::zero();
        let (root, db) = {
            let mut state = get_temp_state();
            state.inc_nonce(&a).unwrap();
            state.add_balance(&a, U256::from(69u64)).unwrap();
            state.commit().unwrap();
            assert_eq!(state.balance(&a).unwrap(), U256::from(69u64));
            assert_eq!(state.nonce(&a).unwrap(), U256::from(1u64));
            (state.root, state.db)
        };

        let mut state = State::from_existing(db, root).unwrap();
        assert_eq!(state.balance(&a).unwrap(), U256::from(69u64));
        assert_eq!(state.nonce(&a).unwrap(), U256::from(1u64));
    }

    #[test]
    fn remove() {
        let a = Address::zero();
        let mut state = get_temp_state();
        assert_eq!(state.exist(&a).unwrap(), false);
        state.inc_nonce(&a).unwrap();
        assert_eq!(state.exist(&a).unwrap(), true);
        assert_eq!(state.nonce(&a).unwrap(), U256::from(1u64));
        state.kill_contract(&a);
        assert_eq!(state.exist(&a).unwrap(), false);
        assert_eq!(state.nonce(&a).unwrap(), U256::from(0u64));
    }

    #[test]
    fn remove_from_database() {
        let a = Address::zero();
        let (root, db) = {
            let mut state = get_temp_state();
            state.add_balance(&a, U256::from(69u64)).unwrap();
            state.commit().unwrap();
            (state.root, state.db)
        };

        let (root, db) = {
            let mut state = State::from_existing(db, root).unwrap();
            assert_eq!(state.exist(&a).unwrap(), true);
            assert_eq!(state.balance(&a).unwrap(), U256::from(69u64));
            state.kill_contract(&a);
            state.commit().unwrap();
            assert_eq!(state.exist(&a).unwrap(), false);
            assert_eq!(state.balance(&a).unwrap(), U256::from(0u64));
            (state.root, state.db)
        };

        let mut state = State::from_existing(db, root).unwrap();
        assert_eq!(state.exist(&a).unwrap(), false);
        assert_eq!(state.balance(&a).unwrap(), U256::from(0u64));
    }

    #[test]
    fn alter_balance() {
        let mut state = get_temp_state();
        let a = Address::zero();
        let b: Address = 1u64.into();

        state.add_balance(&a, U256::from(69u64)).unwrap();
        assert_eq!(state.balance(&a).unwrap(), U256::from(69u64));
        state.commit().unwrap();
        assert_eq!(state.balance(&a).unwrap(), U256::from(69u64));

        state.sub_balance(&a, U256::from(42u64)).unwrap();
        assert_eq!(state.balance(&a).unwrap(), U256::from(27u64));
        state.commit().unwrap();
        assert_eq!(state.balance(&a).unwrap(), U256::from(27u64));

        state.transfer_balance(&a, &b, U256::from(18)).unwrap();
        assert_eq!(state.balance(&a).unwrap(), U256::from(9u64));
        assert_eq!(state.balance(&b).unwrap(), U256::from(18u64));
        state.commit().unwrap();
        assert_eq!(state.balance(&a).unwrap(), U256::from(9u64));
        assert_eq!(state.balance(&b).unwrap(), U256::from(18u64));
    }

    #[test]
    fn alter_nonce() {
        let mut state = get_temp_state();
        let a = Address::zero();
        state.inc_nonce(&a).unwrap();
        assert_eq!(state.nonce(&a).unwrap(), U256::from(1u64));
        state.inc_nonce(&a).unwrap();
        assert_eq!(state.nonce(&a).unwrap(), U256::from(2u64));
        state.commit().unwrap();
        assert_eq!(state.nonce(&a).unwrap(), U256::from(2u64));
        state.inc_nonce(&a).unwrap();
        assert_eq!(state.nonce(&a).unwrap(), U256::from(3u64));
        state.commit().unwrap();
        assert_eq!(state.nonce(&a).unwrap(), U256::from(3u64));
    }

    #[test]
    fn balance_nonce() {
        let mut state = get_temp_state();
        let a = Address::zero();
        assert_eq!(state.balance(&a).unwrap(), U256::from(0u64));
        assert_eq!(state.nonce(&a).unwrap(), U256::from(0u64));
        state.commit().unwrap();
        assert_eq!(state.balance(&a).unwrap(), U256::from(0u64));
        assert_eq!(state.nonce(&a).unwrap(), U256::from(0u64));
    }

    #[test]
    fn ensure_cached() {
        let mut state = get_temp_state();
        let a = Address::zero();
        state.new_contract(&a, U256::from(0u64), U256::from(0u64), None);
        state.commit().unwrap();
        assert_eq!(
            state.root,
            "3d019704df60561fb4ead78a6464021016353c761f2699851994e729ab95ef80".into()
        );
    }

    #[test]
    fn checkpoint_basic() {
        let mut state = get_temp_state();
        let a = Address::zero();

        state.checkpoint();
        state.add_balance(&a, U256::from(69u64)).unwrap();
        assert_eq!(state.balance(&a).unwrap(), U256::from(69u64));
        state.discard_checkpoint();
        assert_eq!(state.balance(&a).unwrap(), U256::from(69u64));

        state.checkpoint();
        state.add_balance(&a, U256::from(1u64)).unwrap();
        assert_eq!(state.balance(&a).unwrap(), U256::from(70u64));
        state.revert_checkpoint();
        assert_eq!(state.balance(&a).unwrap(), U256::from(69u64));
    }

    #[test]
    fn checkpoint_nested() {
        let mut state = get_temp_state();
        let a = Address::zero();
        state.checkpoint();
        state.checkpoint();
        state.add_balance(&a, U256::from(69u64)).unwrap();
        assert_eq!(state.balance(&a).unwrap(), U256::from(69u64));
        state.discard_checkpoint();
        assert_eq!(state.balance(&a).unwrap(), U256::from(69u64));
        state.revert_checkpoint();
        assert_eq!(state.balance(&a).unwrap(), U256::from(0));
    }

    #[test]
    fn checkpoint_revert_to_get_storage() {
        let mut state = get_temp_state();
        let a = Address::zero();
        let k = H256::from(U256::from(0));

        state.checkpoint();
        state.checkpoint();
        state.set_storage(&a, k, H256::from(1u64)).unwrap();
        assert_eq!(state.get_storage(&a, &k).unwrap(), H256::from(1u64));
        state.revert_checkpoint();
        assert!(state.get_storage(&a, &k).unwrap().is_zero());
    }

    #[test]
    fn checkpoint_kill_account() {
        let mut state = get_temp_state();
        let a = Address::zero();
        let k = H256::from(U256::from(0));
        state.checkpoint();
        state.set_storage(&a, k, H256::from(U256::from(1))).unwrap();
        state.checkpoint();
        state.kill_contract(&a);
        assert!(state.get_storage(&a, &k).unwrap().is_zero());
        state.revert_checkpoint();
        assert_eq!(
            state.get_storage(&a, &k).unwrap(),
            H256::from(U256::from(1))
        );
    }

    #[test]
    fn checkpoint_create_contract_fail() {
        let mut state = get_temp_state();
        let orig_root = state.root;
        let a: Address = 1000.into();

        state.checkpoint(); // c1
        state.new_contract(&a, U256::zero(), U256::zero(), None);
        state.add_balance(&a, U256::from(1)).unwrap();
        state.checkpoint(); // c2
        state.add_balance(&a, U256::from(1)).unwrap();
        state.discard_checkpoint(); // discard c2
        state.revert_checkpoint(); // revert to c1
        assert_eq!(state.exist(&a).unwrap(), false);
        state.commit().unwrap();
        assert_eq!(orig_root, state.root);
    }

    #[test]
    fn create_contract_fail_previous_storage() {
        let mut state = get_temp_state();
        let a: Address = 1000.into();
        let k = H256::from(U256::from(0));

        state
            .set_storage(&a, k, H256::from(U256::from(0xffff)))
            .unwrap();
        state.commit().unwrap();
        state.clear();

        let orig_root = state.root;
        assert_eq!(
            state.get_storage(&a, &k).unwrap(),
            H256::from(U256::from(0xffff))
        );
        state.clear();

        state.checkpoint(); // c1
        state.new_contract(&a, U256::zero(), U256::zero(), None);
        state.checkpoint(); // c2
        state.set_storage(&a, k, H256::from(U256::from(2))).unwrap();
        state.revert_checkpoint(); // revert to c2
        assert_eq!(
            state.get_storage(&a, &k).unwrap(),
            H256::from(U256::from(0))
        );
        state.revert_checkpoint(); // revert to c1
        assert_eq!(
            state.get_storage(&a, &k).unwrap(),
            H256::from(U256::from(0xffff))
        );

        state.commit().unwrap();
        assert_eq!(orig_root, state.root);
    }

    #[test]
    fn checkpoint_chores() {
        let mut state = get_temp_state();
        let a: Address = 1000.into();
        let b: Address = 2000.into();
        state.new_contract(&a, 5.into(), 0.into(), Some(vec![10u8, 20, 30, 40, 50]));
        state.add_balance(&a, 5.into()).unwrap();
        state.set_storage(&a, 10.into(), 10.into()).unwrap();
        assert_eq!(state.code(&a).unwrap(), vec![10u8, 20, 30, 40, 50]);
        assert_eq!(state.balance(&a).unwrap(), 10.into());
        assert_eq!(state.get_storage(&a, &10.into()).unwrap(), 10.into());
        state.commit().unwrap();
        let orig_root = state.root;

        // Top         => account_a: balance=8, nonce=0, code=[10, 20, 30, 40, 50],
        //             |      stroage = { 10=15, 20=20 }
        //             |  account_b: balance=30, nonce=0, code=[]
        //             |      storage = { 55=55 }
        //
        //
        // Checkpoint2 => account_a: balance=8, nonce=0, code=[10, 20, 30, 40, 50],
        //             |      stroage = { 10=10, 20=20 }
        //             |  account_b: None
        //
        // Checkpoint1 => account_a: balance=10, nonce=0, code=[10, 20, 30, 40, 50],
        //             |      storage = { 10=10 }
        //             |  account_b: None

        state.checkpoint(); // c1
        state.sub_balance(&a, 2.into()).unwrap();
        state.set_storage(&a, 20.into(), 20.into()).unwrap();
        assert_eq!(state.balance(&a).unwrap(), 8.into());
        assert_eq!(state.get_storage(&a, &10.into()).unwrap(), 10.into());
        assert_eq!(state.get_storage(&a, &20.into()).unwrap(), 20.into());

        state.checkpoint(); // c2
        state.new_contract(&b, 30.into(), 0.into(), None);
        state.set_storage(&a, 10.into(), 15.into()).unwrap();
        assert_eq!(state.balance(&b).unwrap(), 30.into());
        assert_eq!(state.code(&b).unwrap(), vec![]);

        state.revert_checkpoint(); // revert c2
        assert_eq!(state.balance(&a).unwrap(), 8.into());
        assert_eq!(state.get_storage(&a, &10.into()).unwrap(), 10.into());
        assert_eq!(state.get_storage(&a, &20.into()).unwrap(), 20.into());
        assert_eq!(state.balance(&b).unwrap(), 0.into());
        assert_eq!(state.code(&b).unwrap(), vec![]);
        assert_eq!(state.exist(&b).unwrap(), false);

        state.revert_checkpoint(); // revert c1
        assert_eq!(state.code(&a).unwrap(), vec![10u8, 20, 30, 40, 50]);
        assert_eq!(state.balance(&a).unwrap(), 10.into());
        assert_eq!(state.get_storage(&a, &10.into()).unwrap(), 10.into());

        state.commit().unwrap();
        assert_eq!(orig_root, state.root);
    }
}
