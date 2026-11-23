//! End-to-end encryption of direct messages, hybrid and post-quantum.
//!
//! Every user has two long-term encryption keys besides the Ed25519 identity:
//! an X25519 key and an ML-KEM-768 key (NIST FIPS 203, believed safe against
//! quantum computers). Both public halves are published in a [`KeyBundle`]
//! signed with the identity key.
//!
//! A message to a friend is sealed for that friend alone:
//! * a fresh X25519 key pair → shared secret 1 with the friend's X25519 key;
//! * ML-KEM-768 encapsulation to the friend's ML-KEM key → shared secret 2;
//! * key = HKDF-SHA256(secret 1 ‖ secret 2, both IDs, both public values);
//! * XChaCha20-Poly1305 with a random 24-byte nonce.
//!
//! Reading it needs *both* secrets, so a recording made today stays closed
//! even if one of the two schemes is broken later ("harvest now, decrypt
//! later"). Relays, keepers of offline mail and anyone on the gossip topic see
//! only who writes to whom, when, and the size — not the text.
//!
//! The inner message is the usual signed `ChatEvent`, so the reader also checks
//! that the friend wrote it.

use base64::Engine;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use libp2p::identity::Keypair;
use ml_kem::kem::{Decapsulate, Encapsulate};
use ml_kem::{EncodedSizeUser, KemCore, MlKem768};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

type Dk = <MlKem768 as KemCore>::DecapsulationKey;
type Ek = <MlKem768 as KemCore>::EncapsulationKey;

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

/// My secret encryption keys (kept in `e2e.key` in the data folder).
pub struct Secrets {
    x: x25519_dalek::StaticSecret,
    dk: Dk,
    ek: Ek,
}

#[derive(Serialize, Deserialize)]
struct SecretsFile {
    x25519: String,
    mlkem_dk: String,
    mlkem_ek: String,
}

impl Secrets {
    pub fn generate() -> Self {
        let mut rng = rand::thread_rng();
        let x = x25519_dalek::StaticSecret::random_from_rng(&mut rng);
        let (dk, ek) = MlKem768::generate(&mut rng);
        Secrets { x, dk, ek }
    }

    /// Load from the data folder, or make and save new keys.
    pub fn load_or_create() -> Self {
        let path = crate::store::data_dir().join("e2e.key");
        if let Some(s) = std::fs::read_to_string(&path).ok().and_then(|t| Self::from_json(&t)) {
            return s;
        }
        let s = Self::generate();
        let _ = std::fs::create_dir_all(crate::store::data_dir());
        let _ = std::fs::write(&path, s.to_json());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        s
    }

    fn to_json(&self) -> String {
        serde_json::to_string(&SecretsFile {
            x25519: b64().encode(self.x.to_bytes()),
            mlkem_dk: b64().encode(self.dk.as_bytes()),
            mlkem_ek: b64().encode(self.ek.as_bytes()),
        })
        .unwrap_or_default()
    }

    fn from_json(t: &str) -> Option<Self> {
        let f: SecretsFile = serde_json::from_str(t).ok()?;
        let xb: [u8; 32] = b64().decode(f.x25519).ok()?.try_into().ok()?;
        let dkb = b64().decode(f.mlkem_dk).ok()?;
        let ekb = b64().decode(f.mlkem_ek).ok()?;
        let dk = Dk::from_bytes(&dkb.as_slice().try_into().ok()?);
        let ek = Ek::from_bytes(&ekb.as_slice().try_into().ok()?);
        Some(Secrets { x: x25519_dalek::StaticSecret::from(xb), dk, ek })
    }

    /// My public keys, signed with my identity.
    pub fn bundle(&self, identity: &Keypair) -> KeyBundle {
        let mut b = KeyBundle {
            peer: identity.public().to_peer_id().to_base58(),
            x25519: b64().encode(x25519_dalek::PublicKey::from(&self.x).as_bytes()),
            mlkem: b64().encode(self.ek.as_bytes()),
            ts_ms: crate::types::now_ms(),
            sig: String::new(),
        };
        if let Ok(s) = identity.sign(&b.signing_bytes()) {
            b.sig = b64().encode(s);
        }
        b
    }
}

/// Someone's public encryption keys, signed by their identity key.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KeyBundle {
    pub peer: String,
    /// X25519 public key (base64, 32 bytes).
    pub x25519: String,
    /// ML-KEM-768 encapsulation key (base64, 1184 bytes).
    pub mlkem: String,
    pub ts_ms: u64,
    pub sig: String,
}

impl KeyBundle {
    fn signing_bytes(&self) -> Vec<u8> {
        format!("cybercord/keys/v1\n{}\n{}\n{}\n{}", self.peer, self.x25519, self.mlkem, self.ts_ms).into_bytes()
    }

    /// Signed by the identity whose peer ID it names.
    pub fn verify(&self) -> bool {
        let Some(pk) = crate::types::public_key_of(&self.peer) else { return false };
        let Ok(sig) = b64().decode(&self.sig) else { return false };
        pk.verify(&self.signing_bytes(), &sig)
    }

    fn keys(&self) -> Option<(x25519_dalek::PublicKey, Ek)> {
        let x: [u8; 32] = b64().decode(&self.x25519).ok()?.try_into().ok()?;
        let e = b64().decode(&self.mlkem).ok()?;
        Some((x25519_dalek::PublicKey::from(x), Ek::from_bytes(&e.as_slice().try_into().ok()?)))
    }
}

/// A sealed message: only `to` can open it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Sealed {
    pub from: String,
    pub to: String,
    /// Message id (lets keepers of offline mail drop duplicates without reading).
    pub id: String,
    /// Ephemeral X25519 public key (base64).
    pub epk: String,
    /// ML-KEM-768 ciphertext (base64, 1088 bytes).
    pub kem: String,
    pub nonce: String,
    pub ct: String,
}

fn derive(ss1: &[u8], ss2: &[u8], from: &str, to: &str, epk: &[u8], kem: &[u8]) -> [u8; 32] {
    let mut ikm = Vec::with_capacity(64);
    ikm.extend_from_slice(ss1);
    ikm.extend_from_slice(ss2);
    let mut info = format!("cybercord/dm/v1\n{from}\n{to}\n").into_bytes();
    info.extend_from_slice(epk);
    info.extend_from_slice(kem);
    let hk = Hkdf::<Sha256>::new(Some(b"cybercord-hybrid-x25519-mlkem768"), &ikm);
    let mut key = [0u8; 32];
    let _ = hk.expand(&info, &mut key);
    key
}

/// Pad to a bucket so the size says little about the text (256 B steps).
fn pad(mut v: Vec<u8>) -> Vec<u8> {
    let len = v.len() as u32;
    let total = (v.len() + 4).div_ceil(256) * 256;
    v.resize(total - 4, 0);
    v.extend_from_slice(&len.to_le_bytes());
    v
}

fn unpad(mut v: Vec<u8>) -> Option<Vec<u8>> {
    if v.len() < 4 {
        return None;
    }
    let n = u32::from_le_bytes(v[v.len() - 4..].try_into().ok()?) as usize;
    if n > v.len() - 4 {
        return None;
    }
    v.truncate(n);
    Some(v)
}

/// Seal `plain` from `from` to the owner of `bundle`.
pub fn seal(from: &str, bundle: &KeyBundle, id: &str, plain: &[u8]) -> Option<Sealed> {
    let (xpub, ek) = bundle.keys()?;
    let mut rng = rand::thread_rng();
    let eph = x25519_dalek::EphemeralSecret::random_from_rng(&mut rng);
    let epk = x25519_dalek::PublicKey::from(&eph);
    let ss1 = eph.diffie_hellman(&xpub);
    let (ct2, ss2) = ek.encapsulate(&mut rng).ok()?;
    let key = derive(ss1.as_bytes(), ss2.as_slice(), from, &bundle.peer, epk.as_bytes(), ct2.as_slice());
    let mut nonce = [0u8; 24];
    rand::RngCore::fill_bytes(&mut rng, &mut nonce);
    let ct = XChaCha20Poly1305::new((&key).into()).encrypt(XNonce::from_slice(&nonce), pad(plain.to_vec()).as_slice()).ok()?;
    Some(Sealed {
        from: from.to_string(),
        to: bundle.peer.clone(),
        id: id.to_string(),
        epk: b64().encode(epk.as_bytes()),
        kem: b64().encode(ct2.as_slice()),
        nonce: b64().encode(nonce),
        ct: b64().encode(ct),
    })
}

/// Open a message sealed to me.
pub fn open(me: &Secrets, my_id: &str, s: &Sealed) -> Option<Vec<u8>> {
    if s.to != my_id {
        return None;
    }
    let epk: [u8; 32] = b64().decode(&s.epk).ok()?.try_into().ok()?;
    let kem = b64().decode(&s.kem).ok()?;
    let nonce = b64().decode(&s.nonce).ok()?;
    if nonce.len() != 24 {
        return None;
    }
    let ss1 = me.x.diffie_hellman(&x25519_dalek::PublicKey::from(epk));
    let ct2: ml_kem::Ciphertext<MlKem768> = kem.as_slice().try_into().ok()?;
    let ss2 = me.dk.decapsulate(&ct2).ok()?;
    let key = derive(ss1.as_bytes(), ss2.as_slice(), &s.from, &s.to, &epk, &kem);
    let ct = b64().decode(&s.ct).ok()?;
    let plain = XChaCha20Poly1305::new((&key).into()).decrypt(XNonce::from_slice(&nonce), ct.as_slice()).ok()?;
    unpad(plain)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_and_open() {
        let alice_id = Keypair::generate_ed25519();
        let bob_id = Keypair::generate_ed25519();
        let bob = Secrets::generate();
        let bundle = bob.bundle(&bob_id);
        assert!(bundle.verify());
        let a = alice_id.public().to_peer_id().to_base58();
        let b = bob_id.public().to_peer_id().to_base58();
        let s = seal(&a, &bundle, "m1", "Привет, Боб! Квантовый компьютер это не прочтёт.".as_bytes()).unwrap();
        // the size is padded
        assert!(b64().decode(&s.ct).unwrap().len() % 256 == 16);
        let plain = open(&bob, &b, &s).unwrap();
        assert_eq!(String::from_utf8(plain).unwrap(), "Привет, Боб! Квантовый компьютер это не прочтёт.");
        // someone else cannot open it
        let eve = Secrets::generate();
        assert!(open(&eve, &b, &s).is_none());
        // tampering is detected
        let mut t = s.clone();
        t.from = "someone-else".into();
        assert!(open(&bob, &b, &t).is_none());
        // a forged bundle is rejected
        let mut fake = bundle.clone();
        fake.x25519 = eve.bundle(&bob_id).x25519;
        assert!(!fake.verify());
        // keys survive a save / load
        let back = Secrets::from_json(&bob.to_json()).unwrap();
        assert!(open(&back, &b, &s).is_some());
    }
}
