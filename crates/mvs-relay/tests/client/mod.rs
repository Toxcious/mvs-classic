//! The test client of the relay's wire checks (wire.rs, and cross.rs against a relay of another build): a TCP
//! connection whose frames go in a queue, waited for by a predicate, and a UDP socket. It seals as a client does
//! (wire 7): its reader derives the connection's keys at the challenge (from the last hello's key, nonce and kx) and
//! opens what follows it; what it sends is sealed from its prove on ([`Client::write_raw`] and [`Client::udp_raw`]
//! send bytes as they are).
#![allow(dead_code)]

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::pin::pin;
use std::rc::Rc;
use std::task::Poll;
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use mvs_protocol::match_wire::{
    ALL_TICKS, INPUT_BYTES, INPUT_HEADER, MAX_SLOTS, MSG_INPUT, MSG_RULED, packet_fits, runs,
};
use mvs_protocol::relay_seal::{DatagramOpener, KxSecret, SealedReader, TxSeal, datagram_token, derive, shared, split};
use mvs_protocol::relay_wire::{
    FT_JSON, FT_MATCH, Frame, FrameReader, RELAY_WIRE_VERSION, Side, TOKEN_BYTES, Transcript, UDP_PROBE,
    handshake_bytes, json_frame, read_json, read_packet, read_udp_body,
};
use mvs_relay::server::key_id;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::{Notify, mpsc};
use tokio::task::JoinHandle;

/// Runs a check on a current-thread runtime, as the relay runs.
pub fn run(f: impl Future<Output = ()>) {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    tokio::task::LocalSet::new().block_on(&rt, f);
}

pub async fn sleep(ms: u64) {
    tokio::time::sleep(Duration::from_millis(ms)).await;
}
pub async fn until(what: &str, ms: u64, mut cond: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_millis(ms);
    while !cond() {
        assert!(Instant::now() < end, "condition not met: {what}");
        sleep(10).await;
    }
}

pub struct Got {
    pub ty: u8,
    pub body: Vec<u8>,
    pub m: Option<Value>,
}

#[derive(Clone)]
pub struct Dg {
    pub token: [u8; TOKEN_BYTES],
    pub kind: u8,
    pub handle: u32,
    pub bytes: Vec<u8>,
}

#[derive(Default)]
pub struct Inner {
    pub got: Vec<Got>,
    pub closed: bool,
    pub datagrams: Vec<Dg>,
    pub paused: bool,
    /// A write of its failed (the relay's socket is gone).
    pub write_failed: bool,
    /// Every byte the relay sent on TCP, as it came.
    pub raw: Vec<u8>,
    /// The last hello's key, nonce and X25519 secret, until the challenge takes them.
    pub hello: Option<(Vec<u8>, [u8; 32], KxSecret)>,
    /// What this client sends with, once a challenge gave the keys.
    pub tx: Option<TxSeal>,
    /// Its frames are sealed (its prove was sent).
    pub sealing: bool,
    /// What the relay's datagrams are opened with, once a challenge gave the keys.
    pub datagram_opener: Option<DatagramOpener>,
    /// A sealed frame from the relay did not open.
    pub bad_seal: bool,
}

/// The test client's reader of the relay's frames: plain, then sealed from the frame after the challenge.
enum Reader {
    Plain(FrameReader),
    Sealed(SealedReader),
}

/// The keys of a challenge for the hello that asked for it, put in `i`: the opener of the frames after it.
fn keys_of(i: &mut Inner, f: &Frame) -> Option<mvs_protocol::relay_seal::FrameOpener> {
    let m = read_json(&f.body)?;
    if m.get("t")?.as_str()? != "challenge" {
        return None;
    }
    let (key, nonce, kx) = i.hello.take()?;
    let field = |k: &str| -> Option<[u8; 32]> { STANDARD.decode(m.get(k)?.as_str()?).ok()?.try_into().ok() };
    let t = Transcript {
        client_key: key.try_into().ok()?,
        relay_key: field("key")?,
        client_nonce: nonce,
        relay_nonce: field("nonce")?,
        client_kx: kx.public(),
        relay_kx: field("kx")?,
    };
    let secret = shared(kx, &t.relay_kx)?;
    let (tx, opener, datagrams) = split(&derive(&secret, &t), Side::Client);
    i.tx = Some(tx);
    i.datagram_opener = Some(datagrams);
    Some(opener)
}

pub struct Welcome {
    pub id: String,
    pub token: [u8; TOKEN_BYTES],
    pub udp: u16,
    /// The count of channel-0 data frames the relay took from the seat (None: a new seat).
    pub resumed: Option<u64>,
}

/// A test client: frames in a queue, waited for by a predicate.
pub struct Client {
    pub key: SigningKey,
    pub raw: [u8; 32],
    pub inner: Rc<RefCell<Inner>>,
    pub wake: Rc<Notify>,
    pub resume: Rc<Notify>,
    pub out: RefCell<Option<mpsc::UnboundedSender<Vec<u8>>>>,
    pub welcome: RefCell<Option<Welcome>>,
    /// Its UDP socket (a std clone sends: tokio's try_send_to refuses until it has seen the socket writable).
    pub udp: RefCell<Option<std::net::UdpSocket>>,
    pub relay_port: u16,
    pub req_id: Cell<i64>,
    pub tasks: RefCell<Vec<JoinHandle<()>>>,
    /// The last hello's X25519 public key (kx).
    pub kx_public: Cell<[u8; 32]>,
}

pub fn new_key() -> SigningKey {
    let mut s = [0u8; 32];
    rand::fill(&mut s);
    SigningKey::from_bytes(&s)
}

pub fn b64(b: &[u8]) -> String {
    STANDARD.encode(b)
}

pub fn unb64(v: &Value) -> Vec<u8> {
    STANDARD.decode(v.as_str().expect("base64 text")).expect("base64")
}

/// The first of two futures to finish (true: the first).
pub async fn first<A: Future, B: Future>(a: A, b: B) -> bool {
    let (mut a, mut b) = (pin!(a), pin!(b));
    std::future::poll_fn(|cx| {
        if a.as_mut().poll(cx).is_ready() {
            return Poll::Ready(true);
        }
        b.as_mut().poll(cx).map(|_| false)
    })
    .await
}

impl Client {
    pub async fn new(port: u16, key: Option<SigningKey>) -> Rc<Client> {
        let key = key.unwrap_or_else(new_key);
        let raw = key.verifying_key().to_bytes();
        let sock = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let (mut r, mut w) = sock.into_split();
        let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let c = Rc::new(Client {
            key,
            raw,
            inner: Rc::default(),
            wake: Rc::default(),
            resume: Rc::default(),
            out: RefCell::new(Some(tx)),
            welcome: RefCell::new(None),
            udp: RefCell::new(None),
            relay_port: port,
            req_id: Cell::new(0),
            tasks: RefCell::new(Vec::new()),
            kx_public: Cell::new([0; 32]),
        });
        // the other side's end ends this side too (Node's allowHalfOpen false)
        let ended = Rc::new(Notify::new());
        let (inner, wake, resume, ended2) = (c.inner.clone(), c.wake.clone(), c.resume.clone(), ended.clone());
        let reader = tokio::task::spawn_local(async move {
            let mut fr = Reader::Plain(FrameReader::new());
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                while inner.borrow().paused {
                    resume.notified().await;
                }
                match r.read(&mut buf).await {
                    Ok(n) if n > 0 => {
                        let mut i = inner.borrow_mut();
                        i.raw.extend_from_slice(&buf[..n]);
                        match &mut fr {
                            Reader::Plain(p) => p.feed(&buf[..n]),
                            Reader::Sealed(s) => s.feed(&buf[..n]),
                        }
                        loop {
                            let f = match &mut fr {
                                Reader::Plain(p) => p.next(),
                                Reader::Sealed(s) => s.next(),
                            };
                            let Some(f) = f else { break };
                            // (what follows the challenge is sealed)
                            if let Reader::Plain(p) = &mut fr
                                && let Some(opener) = keys_of(&mut i, &f)
                            {
                                fr = Reader::Sealed(SealedReader::new(opener, p.take_rest()));
                            }
                            let m = if f.ty == FT_JSON { read_json(&f.body).map(Value::Object) } else { None };
                            i.got.push(Got { ty: f.ty, body: f.body, m });
                        }
                        if let Reader::Sealed(s) = &fr {
                            i.bad_seal |= s.bad;
                        }
                        drop(i);
                        wake.notify_waiters();
                    }
                    _ => break,
                }
            }
            inner.borrow_mut().closed = true;
            wake.notify_waiters();
            ended2.notify_one();
        });
        let inner = c.inner.clone();
        let writer = tokio::task::spawn_local(async move {
            loop {
                let mut got = None;
                let ended_now = first(ended.notified(), async { got = Some(rx.recv().await) }).await;
                match got {
                    Some(Some(b)) if !ended_now => {
                        if w.write_all(&b).await.is_err() {
                            inner.borrow_mut().write_failed = true;
                            return;
                        }
                    }
                    _ => {
                        let _ = w.shutdown().await;
                        return;
                    }
                }
            }
        });
        c.tasks.borrow_mut().extend([reader, writer]);
        c
    }

    /// Sends one plain frame's bytes: sealed once this client's prove is sent.
    pub fn write(&self, b: &[u8]) {
        let mut i = self.inner.borrow_mut();
        let i = &mut *i;
        if i.sealing
            && let Some(tx) = i.tx.as_mut()
        {
            let mut out = Vec::new();
            tx.seal_frame(b, &mut out).expect("a frame to seal");
            self.write_raw(&out);
            return;
        }
        self.write_raw(b);
    }

    /// Sends bytes as they are.
    pub fn write_raw(&self, b: &[u8]) {
        if let Some(tx) = self.out.borrow().as_ref() {
            let _ = tx.send(b.to_vec());
        }
    }

    /// A control message; a prove's frames after it are sealed.
    pub fn send(&self, m: Value) {
        self.write(&json_frame(&m).unwrap());
        if m["t"] == "prove" {
            self.inner.borrow_mut().sealing = true;
        }
    }

    /// The frame `ty` of `body` sealed as this client's next (taking its counter), not sent.
    pub fn sealed_frame(&self, ty: u8, body: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        self.inner.borrow_mut().tx.as_mut().expect("keys").frame_into(&mut out, ty, &[body]).unwrap();
        out
    }

    pub fn closed(&self) -> bool {
        self.inner.borrow().closed
    }

    pub fn had(&self) -> String {
        let i = self.inner.borrow();
        let v: Vec<String> =
            i.got.iter().map(|g| g.m.as_ref().map_or(g.ty.to_string(), |m| m["t"].to_string())).collect();
        v.join(", ")
    }

    /// Waits for `f` to give something; `None` after `ms`.
    pub async fn wait<T>(&self, ms: u64, mut f: impl FnMut(&mut Inner) -> Option<T>) -> Option<T> {
        let end = Instant::now() + Duration::from_millis(ms);
        loop {
            let n = self.wake.notified();
            let mut n = pin!(n);
            n.as_mut().enable();
            if let Some(x) = f(&mut self.inner.borrow_mut()) {
                return Some(x);
            }
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return None;
            }
            let _ = tokio::time::timeout(left.min(Duration::from_millis(50)), n).await;
        }
    }

    /// The first frame (taken from the queue) that `pred` takes.
    pub async fn next(&self, pred: impl Fn(&Got) -> bool) -> Got {
        let g = self.next_until_closed(2000, pred).await;
        g.unwrap_or_else(|| panic!("closed or timed out; had {}", self.had()))
    }

    /// As `next`, but `None` as soon as the connection is closed with nothing that `pred` takes.
    pub async fn next_until_closed(&self, ms: u64, pred: impl Fn(&Got) -> bool) -> Option<Got> {
        self.wait(ms, |i| {
            if let Some(k) = i.got.iter().position(&pred) {
                return Some(Some(i.got.remove(k)));
            }
            if i.closed { Some(None) } else { None }
        })
        .await
        .flatten()
    }

    pub async fn msg(&self, t: &str) -> Value {
        self.msg_in(t, 2000).await
    }

    pub async fn msg_in(&self, t: &str, ms: u64) -> Value {
        let g = self.next_until_closed(ms, |g| g.m.as_ref().is_some_and(|m| m["t"] == t)).await;
        g.unwrap_or_else(|| panic!("no {t}; closed {}; had {}", self.closed(), self.had())).m.unwrap()
    }

    /// Waits for the close; the refused { why } it got first.
    pub async fn refused(&self) -> String {
        let m = self.msg_in("refused", 3000).await;
        self.closed_within(3000).await;
        m["why"].as_str().unwrap().to_string()
    }

    pub async fn closed_within(&self, ms: u64) {
        assert!(self.wait(ms, |i| i.closed.then_some(())).await.is_some(), "not closed");
    }

    pub fn hello_with(&self, v: Value, key: Option<&[u8]>, nonce: Option<[u8; 32]>) -> [u8; 32] {
        self.hello_got(v, key, nonce, None)
    }

    /// A hello with the resume's count (`got`: the channel-0 data frames this key took on its seat).
    pub fn hello_got(&self, v: Value, key: Option<&[u8]>, nonce: Option<[u8; 32]>, got: Option<u64>) -> [u8; 32] {
        let nonce = nonce.unwrap_or_else(|| {
            let mut n = [0u8; 32];
            rand::fill(&mut n);
            n
        });
        let kx = KxSecret::fresh();
        self.kx_public.set(kx.public());
        let key_sent = key.unwrap_or(&self.raw).to_vec();
        {
            let mut i = self.inner.borrow_mut();
            i.hello = Some((key_sent, nonce, kx));
            i.tx = None;
            i.sealing = false;
            i.datagram_opener = None;
        }
        let mut m = json!({
            "t": "hello",
            "v": v,
            "key": b64(key.unwrap_or(&self.raw)),
            "nonce": b64(&nonce),
            "kx": b64(&self.kx_public.get()),
            "app": "test 1",
        });
        if let Some(g) = got {
            m["got"] = json!(g);
        }
        self.send(m);
        nonce
    }

    pub fn hello(&self) -> [u8; 32] {
        self.hello_with(json!(RELAY_WIRE_VERSION), None, None)
    }

    /// The start's transcript of this client's key, nonce and kx and a challenge.
    pub fn transcript(&self, nonce: [u8; 32], c: &Value, relay_key: Option<[u8; 32]>) -> Transcript {
        Transcript {
            client_key: self.raw,
            relay_key: relay_key.unwrap_or_else(|| unb64(&c["key"]).try_into().unwrap()),
            client_nonce: nonce,
            relay_nonce: unb64(&c["nonce"]).try_into().unwrap(),
            client_kx: self.kx_public.get(),
            relay_kx: unb64(&c["kx"]).try_into().unwrap(),
        }
    }

    pub fn prove(&self, t: &Transcript) {
        self.send(json!({ "t": "prove", "sig": b64(&self.key.sign(&handshake_bytes(Side::Client, t)).to_bytes()) }));
    }

    pub async fn sign_in(&self, relay_id: &str) -> String {
        self.sign_in_got(relay_id, None).await
    }

    /// A sign-in whose hello carries `got` (a resume).
    pub async fn sign_in_got(&self, relay_id: &str, got: Option<u64>) -> String {
        let nonce = self.hello_got(json!(RELAY_WIRE_VERSION), None, None, got);
        let c = self.msg("challenge").await;
        let relay_raw: [u8; 32] = unb64(&c["key"]).try_into().unwrap();
        assert_eq!(key_id(&relay_raw), relay_id, "the relay key is its id");
        let t = self.transcript(nonce, &c, None);
        let sig = Signature::from_bytes(&unb64(&c["sig"]).try_into().unwrap());
        let rk = VerifyingKey::from_bytes(&relay_raw).unwrap();
        assert!(rk.verify(&handshake_bytes(Side::Relay, &t), &sig).is_ok(), "the relay signed the transcript");
        assert!(self.inner.borrow().tx.is_some(), "the challenge gave the keys");
        self.prove(&t);
        let w = self.msg("welcome").await;
        let id = w["id"].as_str().unwrap().to_string();
        let token = unb64(&w["token"]).try_into().unwrap();
        let udp = w["udp"].as_u64().unwrap() as u16;
        *self.welcome.borrow_mut() = Some(Welcome { id: id.clone(), token, udp, resumed: w["resumed"].as_u64() });
        id
    }

    pub async fn req(&self, mut r: Value) -> Value {
        let id = self.req_id.get() + 1;
        self.req_id.set(id);
        r["t"] = json!("req");
        r["id"] = json!(id);
        self.send(r);
        self.next(|g| g.m.as_ref().is_some_and(|m| m["t"] == "res" && m["id"] == id)).await.m.unwrap()
    }

    pub fn token(&self) -> [u8; TOKEN_BYTES] {
        self.welcome.borrow().as_ref().unwrap().token
    }

    pub async fn open_udp(&self, from: &str) {
        let u = std::net::UdpSocket::bind((from, 0)).unwrap();
        u.set_nonblocking(true).unwrap();
        *self.udp.borrow_mut() = Some(u.try_clone().unwrap());
        let u = UdpSocket::from_std(u).unwrap();
        let (inner, wake) = (self.inner.clone(), self.wake.clone());
        let t = tokio::task::spawn_local(async move {
            let mut buf = vec![0u8; 65536];
            while let Ok((n, _)) = u.recv_from(&mut buf).await {
                let Some(token) = datagram_token(&buf[..n]) else { continue };
                let mut i = inner.borrow_mut();
                let Some(body) = i.datagram_opener.as_mut().and_then(|o| o.open(&mut buf[..n])) else { continue };
                if let Some(d) = read_udp_body(body) {
                    let dg = Dg { token, kind: d.kind, handle: d.handle, bytes: d.bytes.to_vec() };
                    i.datagrams.push(dg);
                    drop(i);
                    wake.notify_waiters();
                }
            }
        });
        self.tasks.borrow_mut().push(t);
    }

    pub fn udp_raw(&self, d: &[u8]) {
        let u = self.udp.borrow();
        u.as_ref().unwrap().send_to(d, SocketAddr::new(IpAddr::from([127, 0, 0, 1]), self.relay_port)).unwrap();
    }

    /// A datagram sealed as this client's next (taking its counter), with `token`, not sent.
    pub fn udp_sealed(&self, token: &[u8; TOKEN_BYTES], kind: u8, parts: &[&[u8]]) -> Vec<u8> {
        let mut out = Vec::new();
        self.inner.borrow_mut().tx.as_mut().expect("keys").datagram_into(&mut out, token, kind, parts);
        out
    }

    /// A sealed datagram: for a UDP_MATCH the handle and bytes.
    pub fn udp_send(&self, kind: u8, handle: u32, bytes: &[u8]) {
        self.udp_raw(&self.udp_sealed(&self.token(), kind, &[&handle.to_be_bytes(), bytes]));
    }

    /// A probe; `heard`: its flag (this client hears the acknowledgements).
    pub fn probe(&self, heard: bool) {
        self.udp_raw(&self.udp_sealed(&self.token(), UDP_PROBE, &[&[u8::from(heard)]]));
    }

    pub async fn datagram(&self, pred: impl Fn(&Dg) -> bool) -> Dg {
        self.wait(2000, |i| i.datagrams.iter().position(&pred).map(|k| i.datagrams.remove(k)))
            .await
            .expect("no datagram")
    }

    pub fn datagrams(&self) -> usize {
        self.inner.borrow().datagrams.len()
    }

    pub fn pause(&self) {
        self.inner.borrow_mut().paused = true;
    }

    /// The welcome's `resumed`.
    pub fn resumed(&self) -> Option<u64> {
        self.welcome.borrow().as_ref().unwrap().resumed
    }

    /// Leaves on purpose: a bye, then the connection closed.
    pub fn end(&self) {
        self.send(json!({ "t": "bye" }));
        self.cut();
    }

    /// Closes the connection (what was written goes first) and the UDP socket, with no bye: the relay keeps the seat.
    pub fn cut(&self) {
        self.out.borrow_mut().take();
        self.udp.borrow_mut().take();
        let mut tasks = self.tasks.borrow_mut();
        // the reader and the UDP reader; the writer ends once it has written what it holds
        for (k, t) in tasks.drain(..).enumerate() {
            if k != 1 {
                t.abort();
            }
        }
    }
}

pub fn unpack(body: &[u8]) -> (u32, Vec<u8>) {
    let p = read_packet(body).unwrap();
    (p.handle, p.bytes.to_vec())
}

pub fn match_bytes(g: &Got) -> Vec<u8> {
    read_packet(&g.body).unwrap().bytes.to_vec()
}

pub fn is_ty(ty: u8) -> impl Fn(&Got) -> bool {
    move |g| g.ty == ty
}

pub fn lobby_of(v: &Value) -> String {
    v["value"]["lobby"].as_str().unwrap().to_string()
}

/// An input packet of a slot for `count` ticks from `tick`, each input's last byte `mark`; acknowledging every ruled
/// tick.
pub fn input(slot: u8, tick: u32, mark: u8, count: u8) -> Vec<u8> {
    let mut p = vec![0u8; INPUT_HEADER + usize::from(count) * INPUT_BYTES];
    p[0] = MSG_INPUT;
    p[1] = slot;
    p[2..6].copy_from_slice(&tick.to_le_bytes());
    p[6] = count;
    for k in 0..MAX_SLOTS {
        p[11 + 4 * k..15 + 4 * k].copy_from_slice(&ALL_TICKS.to_le_bytes());
    }
    for k in 0..usize::from(count) {
        p[INPUT_HEADER + (k + 1) * INPUT_BYTES - 1] = mark;
    }
    p
}

/// The ruled inputs a client has had from the referee `rid` (its handle told by a peer message first), together: by
/// (slot, tick) each input's last byte, a substitution -1.
pub fn wire_ruled(c: &Client, rid: &str, handle: &mut u32) -> BTreeMap<(u8, u32), i32> {
    let mut out = BTreeMap::new();
    for g in &c.inner.borrow().got {
        if let Some(m) = &g.m
            && m["t"] == "peer"
            && m["id"] == rid
        {
            *handle = m["handle"].as_u64().unwrap() as u32;
        }
        if g.ty != FT_MATCH {
            continue;
        }
        let p = read_packet(&g.body).unwrap();
        if p.handle != *handle || p.bytes.first() != Some(&MSG_RULED) || !packet_fits(p.bytes) {
            continue;
        }
        for run in runs(p.bytes) {
            let h = run.head;
            let mut at = 0;
            for k in 0..u32::from(h.count) {
                if h.mask & (1 << k) != 0 {
                    out.insert((h.slot, h.first + k), -1);
                } else {
                    out.insert((h.slot, h.first + k), i32::from(run.inputs[at][INPUT_BYTES - 1]));
                    at += 1;
                }
            }
        }
    }
    out
}
