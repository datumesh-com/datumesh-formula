#![allow(dead_code)]
//! The engine on PostgreSQL for the integration tests (feature `engine`).
//!
//! Every [`Mesh`] has a database of its own, created from `DATABASE_URL` (a role allowed to
//! create databases) and dropped afterwards; it holds the store, the key documents of the
//! hosts it signs for, and a clock the test sets. Keys are the vectors' (SPEC 11): the seed of
//! a host is SHA-256 of `datumesh formula test key: <host>`, and the mesh is
//! `mesh.example.net`, so a computed receipt signed with the vectors' conventions (licence,
//! summary, `issued`) has the vectors' receipt hash.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use datumesh_core::json::{self, Value};
use datumesh_core::{Arg, Args, CellRef, Keys, Receipt, ReceiptBuilder, Scalar, SigningKey, Type};
use datumesh_engine::{Answer, Clock, Engine, EngineBuilder, EngineError};
use datumesh_formula::definition::{define_function, publish, Definition};
use datumesh_formula::runtime::FormulaRuntime;
use datumesh_store::pg_unstable::postgres::{Config, NoTls};
use datumesh_store::{migrate, PgStore, Store};

/// The mesh of the vectors' computed receipts.
pub const MESH: &str = "mesh.example.net";
/// The vectors' computed receipts: licence and summary.
pub const LICENCE: &str = "CC0-1.0";
pub const SUMMARY: &str = "Computed by the Datumesh formula reference evaluator.";
/// The start of every test key's window, and the earliest `issued` of a computed receipt.
pub const KEYS_FROM: &str = "2020-01-01T00:00:00.000000Z";
pub const MIN_INSTANT: &str = "0001-01-01T00:00:00.000000Z";

pub fn admin_url() -> String {
    std::env::var("DATABASE_URL").expect("DATABASE_URL names a PostgreSQL 16 database (see README)")
}

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A database of its own, migrated, dropped when the value is dropped.
pub struct TestDb {
    pub name: String,
    admin: String,
}

impl TestDb {
    pub fn migrated() -> TestDb {
        let admin = admin_url();
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().subsec_nanos();
        let name = format!("dm_formula_{}_{}_{}", std::process::id(), COUNTER.fetch_add(1, Ordering::SeqCst), nanos);
        let config: Config = admin.parse().unwrap();
        let mut c = config.connect(NoTls).unwrap();
        c.batch_execute(&format!("CREATE DATABASE {name}")).unwrap();
        let db = TestDb { name, admin };
        migrate::apply(&db.store()).unwrap();
        db
    }

    pub fn url(&self) -> String {
        let (path, query) = match self.admin.split_once('?') {
            Some((p, q)) => (p, Some(q)),
            None => (self.admin.as_str(), None),
        };
        let (base, _) = path.rsplit_once('/').unwrap();
        match query {
            Some(q) => format!("{base}/{}?{q}", self.name),
            None => format!("{base}/{}", self.name),
        }
    }

    pub fn store(&self) -> PgStore {
        PgStore::connect(&self.url()).unwrap()
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        let admin: Config = self.admin.parse().unwrap();
        if let Ok(mut c) = admin.connect(NoTls) {
            let _ = c.batch_execute(&format!("DROP DATABASE IF EXISTS {} WITH (FORCE)", self.name));
        }
    }
}

/// A clock the test sets.
#[derive(Clone)]
pub struct TestClock(Arc<Mutex<String>>);

impl TestClock {
    pub fn new(now: &str) -> TestClock {
        TestClock(Arc::new(Mutex::new(now.to_string())))
    }
    pub fn set(&self, now: &str) {
        *self.0.lock().unwrap() = now.to_string();
    }
}

impl Clock for TestClock {
    fn now(&self) -> String {
        self.0.lock().unwrap().clone()
    }
}

/// The test key of a host (SPEC 11).
pub fn key_for(host: &str) -> SigningKey {
    let seed = datumesh_core::sha256(format!("datumesh formula test key: {host}").as_bytes());
    SigningKey::from_seed(&seed)
}

/// A key document publishing `host`'s test key from 2020 on (canonical JSON).
pub fn key_doc(host: &str) -> Vec<u8> {
    let key = key_for(host);
    let hex: String = key.public_key().iter().map(|b| format!("{b:02x}")).collect();
    format!(
        r#"{{"host":"{host}","keys":[{{"alg":"ed25519","id":"{}","public":"{hex}","valid_from":"{KEYS_FROM}"}}],"proto":"datumesh/1"}}"#,
        key.key_id()
    )
    .into_bytes()
}

/// The canonical JSON text of a whole receipt (payload plus `sig`).
pub fn receipt_json(r: &Receipt) -> Vec<u8> {
    let mut members = vec![("sig".to_string(), Value::String(r.sig().to_string()))];
    members.extend(json::parse(r.payload()).unwrap().as_object().unwrap().iter().cloned());
    Value::Object(members).to_jcs()
}

pub fn num(s: &str) -> Scalar {
    Scalar::from_lexical(Type::Number, s).unwrap()
}

pub fn text(s: &str) -> Scalar {
    Scalar::from_lexical(Type::String, s).unwrap()
}

pub fn date(s: &str) -> Scalar {
    Scalar::from_lexical(Type::Date, s).unwrap()
}

pub fn instant(s: &str) -> Scalar {
    Scalar::from_lexical(Type::Instant, s).unwrap()
}

pub fn args(pairs: &[(&str, Scalar)]) -> Args {
    pairs.iter().map(|(k, v)| (k.to_string(), Arg::Value(v.clone()))).collect()
}

/// The receipt of an answer; panics on anything else.
pub fn receipt(a: Result<Answer, EngineError>) -> (Vec<u8>, Receipt) {
    match a {
        Ok(Answer::Receipt(bytes)) => {
            let r = Receipt::from_json(&bytes).unwrap();
            (bytes, r)
        }
        other => panic!("expected a receipt, got {other:?}"),
    }
}

/// A store, the key documents of the hosts it signs for, a clock, and the formula runtime.
pub struct Mesh {
    pub db: TestDb,
    pub store: Arc<PgStore>,
    pub keys: Keys,
    pub clock: TestClock,
}

impl Mesh {
    /// A mesh in a fresh database, with the mesh's key document and those of `hosts`.
    pub fn new(hosts: &[&str]) -> Mesh {
        let db = TestDb::migrated();
        let store = Arc::new(db.store());
        let mut m = Mesh { db, store, keys: Keys::new(), clock: TestClock::new("2026-01-01T00:00:00.000000Z") };
        m.add_host(MESH);
        for h in hosts {
            m.add_host(h);
        }
        m
    }

    pub fn add_host(&mut self, host: &str) {
        if self.store.namespace(host).unwrap().is_none() {
            self.store.put_namespace(host, &key_doc(host), KEYS_FROM).unwrap();
        }
        self.keys.insert_response(host, 200, Some(&key_doc(host)));
    }

    pub fn shared(&self) -> Arc<dyn Store + Send + Sync> {
        self.store.clone()
    }

    /// An engine builder with the vectors' conventions and a formula runtime over the store.
    pub fn builder(&self) -> (EngineBuilder, Arc<FormulaRuntime>) {
        let rt = Arc::new(FormulaRuntime::new(self.shared()));
        let b = Engine::builder(self.shared(), MESH)
            .signing_key(key_for(MESH))
            .licence(LICENCE)
            .summary(|_| SUMMARY.to_string())
            .clock(Arc::new(self.clock.clone()))
            .runtime(rt.clone());
        (b, rt)
    }

    pub fn engine(&self) -> (Engine, Arc<FormulaRuntime>) {
        let (b, rt) = self.builder();
        (b.build().unwrap(), rt)
    }

    /// Store a descriptor (JSON text) and bind `name` to it from `tx`.
    pub fn bind(&self, name: &str, descriptor: &[u8], tx: &str) -> String {
        let h = self.store.put_descriptor(descriptor).unwrap();
        self.store.bind_name(name, &h, tx).unwrap();
        h
    }

    /// A stored function with `signature` (JSON text), bound from `tx`.
    pub fn stored(&self, name: &str, signature: &str, tx: &str) -> String {
        let d = format!(r#"{{"code":{{"body":"","lang":"stored"}},"inputs":[],"signature":{signature}}}"#);
        self.bind(name, d.as_bytes(), tx)
    }

    /// Define `source` as `name` with `signature` (JSON text) against the store's bindings as
    /// of `tx`, and publish it from `tx`.
    pub fn define(&self, name: &str, source: &str, signature: &str, tx: &str) -> Definition {
        let sig = json::parse(signature.as_bytes()).unwrap();
        let d = define_function(name, source, &sig, &BTreeMap::new(), &*self.store, tx)
            .unwrap_or_else(|e| panic!("define {name}: {e}"));
        publish(&*self.store, &d, tx).unwrap();
        d
    }

    /// Record the publisher's version of a stored cell: `value` at `tx`, as of `asof`.
    pub fn record(&self, name: &str, cell_args: &[(&str, Scalar)], value: Scalar, tx: &str, asof: &str) -> String {
        let (ns, f) = name.rsplit_once('/').unwrap();
        let fn_hash = self.store.resolve_name(name, tx).unwrap().expect("bound");
        let descriptor = datumesh_core::FunctionDescriptor::from_json(&self.store.descriptor(&fn_hash).unwrap()).unwrap();
        let a = args(cell_args);
        let valid: Args = a
            .iter()
            .filter(|(k, _)| descriptor.signature.args.get(*k).is_some_and(|d| d.valid))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let cell = datumesh_core::Cell { ns: ns.into(), fn_name: f.into(), fn_hash, args: a };
        let issued = if asof > KEYS_FROM { asof } else { KEYS_FROM };
        let r = ReceiptBuilder::new()
            .cell(cell.clone())
            .value(value)
            .valid(valid)
            .asof(asof)
            .tx(tx)
            .issued(issued)
            .publisher(ns)
            .signer_host(ns)
            .licence("CC-BY-4.0")
            .summary(format!("A recorded value of {f}."))
            .sign(&key_for(ns))
            .unwrap();
        let hash = self.store.put_receipt(&receipt_json(&r)).unwrap();
        self.store.put_version(&cell.id(), tx, &hash).unwrap();
        hash
    }

    /// Put a whole receipt (a vector's JSON value) and, for a stored cell, its version.
    pub fn put_receipt_value(&self, r: &Value) -> String {
        let bytes = r.to_jcs();
        let receipt = Receipt::from_json(&bytes).unwrap();
        let hash = self.store.put_receipt(&bytes).unwrap();
        self.store.put_version(&receipt.cell().id(), receipt.tx(), &hash).unwrap();
        hash
    }
}

/// A reference to the cell of `name` with `pairs`.
pub fn cell_ref(name: &str, pairs: &[(&str, Scalar)]) -> CellRef {
    let (ns, f) = name.rsplit_once('/').unwrap();
    CellRef { ns: ns.into(), fn_name: f.into(), args: args(pairs), asof: None }
}
