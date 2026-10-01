//! Save binário — `docs/03-modelo-de-dados.md §8.1`.
//!
//! O formato aqui é deliberadamente menor do que o `[header] | [blocos] |
//! [footer]` completo descrito em `docs/03 §8.1`: hoje uma `GameSession`
//! (`app::GameSession`) não guarda nenhum estado mutável por entidade —
//! sem elenco jogável, contratos, transferências ou conhecimento, não há
//! `people | clubs | contracts | ...` para colunar. O que existe é
//! exatamente o suficiente para reconstruir uma sessão **por replay**:
//! qual pack, qual seed do mundo, e quantas vezes o único `Command` que
//! existe (`app::Command::AdvanceSeason`) já foi despachado. Isso não é um
//! atalho inventado aqui — é a estratégia que `docs/02 §4`/`§8.1` já prevê
//! ("o log de comandos... permite, opcionalmente, reconstruir o save"), só
//! que hoje o "log" é um contador porque só existe um tipo de comando sem
//! parâmetros. Quando `app::Command` ganhar variantes (transferência,
//! escalação, ...), este formato precisa crescer para um log de verdade —
//! `seasons_advanced` deixa de bastar.
//!
//! Layout, todos os inteiros em little-endian (`docs/02 §8.1`: "endianness
//! fixa ⇒ o mesmo arquivo abre no Windows e no Android"):
//!
//! ```text
//! magic            [u8; 10]  "CM0304OPEN"
//! schema_version   u16
//! sim_version      u16       // versão do motor que gerou este save (docs/02 §5, golden masters)
//! world_seed       u64
//! seasons_advanced u32
//! pack_id          (u16 len, bytes utf-8)
//! pack_version     (u16 len, bytes utf-8)
//! checksum         u64       // FNV-1a de tudo antes deste campo
//! ```
//!
//! `checksum` é FNV-1a, não o xxh3 mencionado em `docs/03 §8.1` — xxh3
//! pediria uma dependência nova só para 26-ish bytes de payload; FNV-1a é
//! ~10 linhas, determinístico, sem dependência, e serve exatamente ao
//! mesmo propósito (detectar corrupção) até o dia em que blocos de verdade
//! (comprimidos, grandes) justifiquem trocar de algoritmo.

use std::path::Path;

use crate::error::PersistError;

const MAGIC: [u8; 10] = *b"CM0304OPEN";

/// Versão do formato de save em si (layout dos campos abaixo). Bump exige
/// migração + teste com um save real da versão anterior (`docs/02 §8.3`) —
/// nenhum dos dois existe ainda porque não há uma versão anterior.
pub const SCHEMA_VERSION: u16 = 1;

/// Versão do motor de simulação que produziu este save. Bump 0→1: motor
/// v0.5 recalibrado (`HOME_ADVANTAGE` 1,4→1,25 em `engine::simulate`, ver
/// `docs/04 §4.1`/`§4.2`) — a regra de `docs/08 §3` é "nunca regravar um
/// golden sem bump de `sim_version`"; este é o primeiro bump de verdade.
/// Ainda não é lido por `golden::verify` (`docs/08 §3`, próxima fatia) nem
/// recusa comparar saves de motores diferentes — só guardado por enquanto.
pub const SIM_VERSION: u16 = 1;

/// O conteúdo de um save — o suficiente para `app::GameSession::load`
/// reconstruir a sessão por replay. Ver o doc do módulo para o porquê de
/// não guardar mais do que isto ainda.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveFile {
    /// `pack::PackManifest::id` — `load` recusa abrir contra um pack
    /// diferente (`docs/03 §2`: um save não sobrevive à troca de pack sem
    /// remapeamento, que ainda não existe).
    pub pack_id: String,
    pub pack_version: String,
    pub world_seed: u64,
    pub seasons_advanced: u32,
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET_BASIS;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

fn write_str(buf: &mut Vec<u8>, field: &'static str, s: &str) -> Result<(), PersistError> {
    let bytes = s.as_bytes();
    let len: u16 = bytes
        .len()
        .try_into()
        .map_err(|_| PersistError::FieldTooLong {
            field,
            len: bytes.len(),
        })?;
    buf.extend_from_slice(&len.to_le_bytes());
    buf.extend_from_slice(bytes);
    Ok(())
}

/// Codifica um [`SaveFile`] nos bytes do formato descrito no doc do módulo.
pub fn encode(save: &SaveFile) -> Result<Vec<u8>, PersistError> {
    let mut buf = Vec::with_capacity(64);
    buf.extend_from_slice(&MAGIC);
    buf.extend_from_slice(&SCHEMA_VERSION.to_le_bytes());
    buf.extend_from_slice(&SIM_VERSION.to_le_bytes());
    buf.extend_from_slice(&save.world_seed.to_le_bytes());
    buf.extend_from_slice(&save.seasons_advanced.to_le_bytes());
    write_str(&mut buf, "pack_id", &save.pack_id)?;
    write_str(&mut buf, "pack_version", &save.pack_version)?;

    let checksum = fnv1a64(&buf);
    buf.extend_from_slice(&checksum.to_le_bytes());
    Ok(buf)
}

/// Cursor de leitura minúsculo: cada `take_*` avança o cursor e devolve
/// `PersistError::Truncated` em vez de panicar se não sobrar bytes
/// suficientes — é o único jeito de decodificar dado externo sem violar
/// `docs/02 §9.2` ("núcleo devolve `Result`... `panic` é bug").
struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], PersistError> {
        let end = self.pos.checked_add(n).ok_or(PersistError::Truncated)?;
        let slice = self
            .bytes
            .get(self.pos..end)
            .ok_or(PersistError::Truncated)?;
        self.pos = end;
        Ok(slice)
    }

    fn take_u16(&mut self) -> Result<u16, PersistError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn take_u32(&mut self) -> Result<u32, PersistError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn take_u64(&mut self) -> Result<u64, PersistError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn take_str(&mut self) -> Result<String, PersistError> {
        let len = self.take_u16()? as usize;
        let bytes = self.take(len)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| PersistError::InvalidUtf8)
    }
}

/// Decodifica bytes previamente produzidos por [`encode`]. A ordem de
/// validação importa para dar a mensagem certa (`docs/02 §9.2`: "todo erro
/// de comando vira mensagem acionável"):
///
/// 1. `MAGIC` — antes de qualquer outra coisa, porque um arquivo que não é
///    um save deste jogo (arquivo errado, lixo) deve dizer isso, não
///    "corrompido".
/// 2. `schema_version` — só depois de confirmar que é um save nosso; um
///    `schema_version` desconhecido muda o layout do resto do arquivo, e
///    é lido dos 12 primeiros bytes só, sem presumir nada sobre o
///    tamanho total.
/// 3. **Checksum**, sobre `bytes` inteiro exceto os últimos 8 — antes de
///    decodificar qualquer campo do payload (`world_seed`, as strings...).
///    Isso é o que garante que uma corrupção em qualquer lugar do arquivo
///    vira sempre `ChecksumMismatch`, nunca um erro incidental de parsing
///    (ex.: um byte de texto corrompido virando `InvalidUtf8`) que
///    esconderia a causa real.
/// 4. Só com o checksum batendo os campos são de fato interpretados — um
///    checksum válido não garante comprimentos de string internos
///    consistentes (um arquivo forjado poderia ter checksum certo com
///    tamanhos errados), então `Reader` continua podendo devolver
///    `Truncated` mesmo depois do passo 3.
pub fn decode(bytes: &[u8]) -> Result<SaveFile, PersistError> {
    if bytes.len() < MAGIC.len() + 2 {
        return Err(PersistError::Truncated);
    }
    if bytes[..MAGIC.len()] != MAGIC {
        return Err(PersistError::BadMagic);
    }
    let schema_version = u16::from_le_bytes(
        bytes[MAGIC.len()..MAGIC.len() + 2]
            .try_into()
            .expect("fatia de exatamente 2 bytes"),
    );
    if schema_version != SCHEMA_VERSION {
        return Err(PersistError::UnsupportedSchemaVersion {
            found: schema_version,
        });
    }

    // `bytes.len() >= MAGIC.len() + 2` (checado acima) já garante `>= 8`,
    // então `split_at` abaixo nunca estoura.
    let (payload, checksum_bytes) = bytes.split_at(bytes.len() - 8);
    let checksum = u64::from_le_bytes(
        checksum_bytes
            .try_into()
            .expect("fatia de exatamente 8 bytes"),
    );
    if fnv1a64(payload) != checksum {
        return Err(PersistError::ChecksumMismatch);
    }

    let mut r = Reader::new(payload);
    r.take(MAGIC.len() + 2)?; // magic + schema_version, já validados acima
    let _sim_version = r.take_u16()?; // não afeta a leitura do v1; guardado p/ golden masters futuros.
    let world_seed = r.take_u64()?;
    let seasons_advanced = r.take_u32()?;
    let pack_id = r.take_str()?;
    let pack_version = r.take_str()?;

    Ok(SaveFile {
        pack_id,
        pack_version,
        world_seed,
        seasons_advanced,
    })
}

/// Grava um save em disco com **escrita atômica** (`docs/02 §8.1`):
/// serializa para um arquivo temporário no mesmo diretório e só então
/// renomeia por cima do destino. Um processo interrompido no meio da
/// escrita deixa o `.tmp` órfão (limpável), nunca o save anterior
/// corrompido pela metade — é a garantia que faz autosave seguro.
pub fn save_to_path(path: &Path, save: &SaveFile) -> Result<(), PersistError> {
    let bytes = encode(save)?;
    let tmp_path = path.with_extension("tmp");
    std::fs::write(&tmp_path, &bytes).map_err(|source| PersistError::Io {
        path: tmp_path.clone(),
        source,
    })?;
    std::fs::rename(&tmp_path, path).map_err(|source| PersistError::Io {
        path: path.to_path_buf(),
        source,
    })
}

/// Lê e decodifica um save de disco.
pub fn load_from_path(path: &Path) -> Result<SaveFile, PersistError> {
    let bytes = std::fs::read(path).map_err(|source| PersistError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    decode(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> SaveFile {
        SaveFile {
            pack_id: "example.two-tier".to_string(),
            pack_version: "0.1.0".to_string(),
            world_seed: 42,
            seasons_advanced: 3,
        }
    }

    #[test]
    fn round_trip_encode_decode() {
        let save = sample();
        let bytes = encode(&save).unwrap();
        let decoded = decode(&bytes).unwrap();
        assert_eq!(decoded, save);
    }

    #[test]
    fn bytes_vazios_dao_truncado_nao_panic() {
        assert!(matches!(decode(&[]), Err(PersistError::Truncated)));
    }

    #[test]
    fn magic_errado_e_rejeitado() {
        let mut bytes = encode(&sample()).unwrap();
        bytes[0] = b'X';
        assert!(matches!(decode(&bytes), Err(PersistError::BadMagic)));
    }

    #[test]
    fn schema_version_desconhecido_e_rejeitado() {
        let mut bytes = encode(&sample()).unwrap();
        // offset 10..12 é schema_version (little-endian) — 65535 nunca vai
        // ser um schema válido.
        bytes[10] = 0xFF;
        bytes[11] = 0xFF;
        assert!(matches!(
            decode(&bytes),
            Err(PersistError::UnsupportedSchemaVersion { found: 0xFFFF })
        ));
    }

    #[test]
    fn byte_corrompido_no_meio_e_pego_pelo_checksum() {
        // O checksum é validado antes de qualquer campo ser interpretado
        // (ver a ordem documentada em `decode`), então corromper um byte em
        // qualquer lugar do payload — inclusive dentro de uma string UTF-8
        // — tem que virar `ChecksumMismatch`, nunca um `InvalidUtf8`
        // incidental que esconderia a causa real.
        let mut bytes = encode(&sample()).unwrap();
        let mid = bytes.len() / 2;
        bytes[mid] ^= 0xFF;
        assert!(matches!(
            decode(&bytes),
            Err(PersistError::ChecksumMismatch)
        ));
    }

    #[test]
    fn byte_corrompido_dentro_de_uma_string_tambem_da_checksum_mismatch_nao_invalid_utf8() {
        let mut bytes = encode(&sample()).unwrap();
        // offset 30 cai dentro de `pack_id` ("example.two-tier") — o caso
        // que originalmente vazava `InvalidUtf8` antes de o checksum ser
        // verificado primeiro.
        bytes[30] ^= 0xFF;
        assert!(matches!(
            decode(&bytes),
            Err(PersistError::ChecksumMismatch)
        ));
    }

    #[test]
    fn arquivo_cortado_no_meio_muda_o_tamanho_e_vira_checksum_mismatch() {
        // Cortar bytes do fim desloca onde os últimos 8 bytes (o campo de
        // checksum) caem — o resultado quase sempre é `ChecksumMismatch`,
        // não `Truncated`: o arquivo continua "bem formado" o bastante
        // para passar pelas checagens de tamanho mínimo, só que os bytes
        // não batem mais com o checksum gravado. É um resultado correto:
        // um save cortado é, de fato, um save corrompido.
        let bytes = encode(&sample()).unwrap();
        let truncated = &bytes[..bytes.len() - 5];
        assert!(matches!(
            decode(truncated),
            Err(PersistError::ChecksumMismatch) | Err(PersistError::Truncated)
        ));
    }

    #[test]
    fn comprimento_de_string_maior_que_o_restante_do_arquivo_da_truncado_nao_panic() {
        // Constrói um payload com checksum *correto*, mas com um prefixo
        // de comprimento de `pack_id` maior do que os bytes que de fato
        // seguem — o único jeito de exercitar `Truncated` depois que o
        // checksum já validou o resto do arquivo (`decode`, passo 4).
        let mut buf = Vec::new();
        buf.extend_from_slice(&MAGIC);
        buf.extend_from_slice(&SCHEMA_VERSION.to_le_bytes());
        buf.extend_from_slice(&SIM_VERSION.to_le_bytes());
        buf.extend_from_slice(&0u64.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&500u16.to_le_bytes()); // promete 500 bytes de pack_id...
        // ...mas não escreve nenhum: o arquivo acaba aqui.
        let checksum = fnv1a64(&buf);
        buf.extend_from_slice(&checksum.to_le_bytes());

        assert!(matches!(decode(&buf), Err(PersistError::Truncated)));
    }

    #[test]
    fn campo_maior_que_65535_bytes_e_rejeitado_sem_panic() {
        let mut save = sample();
        save.pack_id = "x".repeat(70_000);
        assert!(matches!(
            encode(&save),
            Err(PersistError::FieldTooLong {
                field: "pack_id",
                ..
            })
        ));
    }

    #[test]
    fn save_e_load_em_disco_fazem_round_trip() {
        let dir =
            std::env::temp_dir().join(format!("managerfc-persist-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("slot0.cm0304save");

        let save = sample();
        save_to_path(&path, &save).unwrap();
        let loaded = load_from_path(&path).unwrap();
        assert_eq!(loaded, save);

        // Escrita atômica não deixa `.tmp` para trás em caso de sucesso.
        assert!(!path.with_extension("tmp").exists());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn carregar_caminho_inexistente_da_erro_de_io_tipado() {
        let err = load_from_path(Path::new("/tmp/managerfc-nao-existe.cm0304save")).unwrap_err();
        assert!(matches!(err, PersistError::Io { .. }));
    }
}

/// Testes de propriedade (`docs/08 §1`).
#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// `decode(encode(x)) == x` para qualquer combinação de campos
        /// dentro dos limites do formato — generaliza `round_trip_encode_decode`
        /// além do único exemplo fixo.
        #[test]
        fn round_trip_para_qualquer_save_valido(
            pack_id in "[a-z.]{1,40}",
            pack_version in "[0-9.]{1,10}",
            world_seed: u64,
            seasons_advanced: u32,
        ) {
            let save = SaveFile { pack_id, pack_version, world_seed, seasons_advanced };
            let bytes = encode(&save).unwrap();
            let decoded = decode(&bytes).unwrap();
            prop_assert_eq!(decoded, save);
        }

        /// `decode` nunca panica, para nenhuma sequência de bytes — só
        /// devolve `Err` ou `Ok`. É a propriedade mais importante deste
        /// módulo: ele processa arquivo em disco, o exemplo canônico de
        /// "dado externo" que `docs/02 §9.2` proíbe de derrubar o núcleo.
        #[test]
        fn decode_nunca_panica_para_bytes_arbitrarios(bytes in prop::collection::vec(any::<u8>(), 0..128)) {
            let _ = decode(&bytes);
        }
    }
}
