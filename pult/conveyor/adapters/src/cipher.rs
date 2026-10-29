//! Расшифровка токена подключения (Story 1.4, AD-16).
//!
//! Write-only vault Story 1.3: шлюз ШИФРУЕТ (EnvAesGcmCipher), прод-путь
//! расшифровки отсутствовал. Первый потребитель decrypt — конвейер: адаптер
//! расшифровывает токен для запросов к площадке. Реализация совместима со
//! схемой шлюза (AES-256-GCM, nonce 12 байт, шифротекст||GCM-тег), что
//! доказывается round-trip-тестом с ручным шифрованием тем же ключом.
//!
//! Инварианты:
//! - расшифрованные байты НЕ логируются (трассировка — только метаданные);
//! - encrypt в этом исполнении НЕ поддерживается (конвейер не пишет в vault,
//!   окно записи шлюза остаётся единственным) — явная ошибка, не заглушка.

use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use pult_core_identity::ports::{MasterKey, TokenCipher};

/// AES-256-GCM расшифровщик токенов конвейера (порт `TokenCipher`).
pub struct VaultTokenCipher {
    key_bytes: [u8; 32],
}

/// Ошибка работы шифратора конвейера.
#[derive(Debug, thiserror::Error)]
pub enum CipherError {
    /// Расшифровка не удалась (неверный ключ/nonce/повреждённые данные).
    #[error("расшифровка токена не удалась (ключ/nonce/данные)")]
    DecryptFailed,
    /// Шифрование в конвейере не поддерживается (окно записи — шлюз).
    #[error("encrypt в конвейере не поддерживается: запись vault — только шлюз")]
    EncryptUnsupported,
}

impl VaultTokenCipher {
    /// Создаёт расшифровщик из мастер-ключа (32 байта).
    pub fn new(master_key: &MasterKey) -> Self {
        Self {
            key_bytes: master_key.bytes,
        }
    }
}

impl TokenCipher for VaultTokenCipher {
    type Error = CipherError;

    fn encrypt(
        &self,
        _plaintext: &[u8],
    ) -> Result<pult_core_identity::ports::EncryptedPayload, Self::Error> {
        Err(CipherError::EncryptUnsupported)
    }

    fn decrypt(
        &self,
        payload: &pult_core_identity::ports::EncryptedPayload,
    ) -> Result<Vec<u8>, Self::Error> {
        let cipher =
            Aes256Gcm::new_from_slice(&self.key_bytes).map_err(|_| CipherError::DecryptFailed)?;
        // Строгая длина nonce (12 байт для GCM) до конструирования — паттерн шлюза.
        let nonce_bytes: [u8; 12] = payload
            .nonce
            .as_slice()
            .try_into()
            .map_err(|_| CipherError::DecryptFailed)?;
        let nonce = Nonce::from(nonce_bytes);
        cipher
            .decrypt(&nonce, payload.ciphertext.as_slice())
            .map_err(|_| CipherError::DecryptFailed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes_gcm::aead::Aead;

    fn master_key(bytes: &[u8; 32]) -> MasterKey {
        MasterKey {
            version: 1,
            bytes: *bytes,
        }
    }

    /// Ручное шифрование СХЕМОЙ ШЛЮЗА (nonce 12 байт, шифротекст||тег) —
    /// round-trip доказывает совместимость без вызова кода шлюза.
    fn encrypt_like_gateway(
        key_bytes: &[u8; 32],
        nonce_bytes: &[u8; 12],
        plaintext: &[u8],
    ) -> pult_core_identity::ports::EncryptedPayload {
        let cipher = Aes256Gcm::new_from_slice(key_bytes).expect("ключ 32 байта валиден");
        let nonce = Nonce::from(*nonce_bytes);
        let ct = cipher
            .encrypt(&nonce, plaintext)
            .expect("ручное шифрование не падает");
        pult_core_identity::ports::EncryptedPayload {
            ciphertext: ct,
            nonce: nonce_bytes.to_vec(),
            key_version: 1,
        }
    }

    #[test]
    fn decrypt_round_trip_with_gateway_scheme() {
        let key = [7u8; 32];
        let nonce = [42u8; 12];
        let secret = b"wb-token-abcdef0123456789".to_vec();
        let payload = encrypt_like_gateway(&key, &nonce, &secret);

        let cipher = VaultTokenCipher::new(&master_key(&key));
        let plain = cipher
            .decrypt(&payload)
            .expect("совместимая схема расшифровывается");
        assert_eq!(plain, secret);
    }

    #[test]
    fn wrong_key_or_nonce_fails_closed() {
        let key = [7u8; 32];
        let payload = encrypt_like_gateway(&key, &[42u8; 12], b"secret".as_ref());

        // неверный ключ
        let cipher = VaultTokenCipher::new(&master_key(&[8u8; 32]));
        assert!(matches!(
            cipher.decrypt(&payload),
            Err(CipherError::DecryptFailed)
        ));

        // повреждённый шифротекст (перевёрнут тег)
        let cipher_ok = VaultTokenCipher::new(&master_key(&key));
        let mut corrupted = payload.clone();
        let n = corrupted.ciphertext.len();
        corrupted.ciphertext[n - 1] ^= 0xff;
        assert!(matches!(
            cipher_ok.decrypt(&corrupted),
            Err(CipherError::DecryptFailed)
        ));
    }

    #[test]
    fn encrypt_is_unsupported_by_design() {
        let cipher = VaultTokenCipher::new(&master_key(&[7u8; 32]));
        assert!(matches!(
            cipher.encrypt(b"x"),
            Err(CipherError::EncryptUnsupported)
        ));
    }
}
