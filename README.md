# ManagerFC — reimplementação aberta de um manager de futebol clássico

> **Codinome do repositório:** `cm0304_open`
> **Nome do produto:** **ManagerFC** (decisão registrada em [`docs/05-dados-e-legal.md`](docs/05-dados-e-legal.md#2-nome-do-produto); busca formal de marca ainda pendente)
> **Status:** 🚧 fase de fundação (M0) — escopo fechado, scaffolding do núcleo em Rust iniciado.

**ManagerFC** recria, de forma **aberta (open source)** e **legalmente limpa**, a
experiência de jogo dos managers de futebol clássicos do início dos anos 2000 —
profundidade de base de dados, ritmo de jogo, visão 2D top-down da partida, atributos
1–20 — para **Windows 10/11**, **Android** e **iOS**, com o mesmo save rodando em todas
as plataformas. As referências de design são o *Championship Manager 03/04* e o
*Elifoot 98* — ver [`docs/10-referencias-motores.md`](docs/10-referencias-motores.md) —
mas nenhum código, asset ou base de dados de nenhum dos dois entra neste repositório.

O repositório está saindo da fase de **apenas escopo** para o **M0 — Fundação**: o
scaffolding do núcleo em Rust já existe (ver [`core/`](core/)); a interface Flutter e o
restante do pipeline de M0 seguem em construção incremental.

## Leia nesta ordem

| # | Documento | O que responde |
|---|-----------|----------------|
| 00 | [Escopo mestre](docs/00-escopo.md) | O que é, o que **não** é, MVP, fases, critérios de sucesso |
| 01 | [Requisitos](docs/01-requisitos.md) | Épicos, features, requisitos não-funcionais, backlog priorizado |
| 02 | [Arquitetura](docs/02-arquitetura.md) | Stack, camadas, determinismo, performance, build multiplataforma |
| 03 | [Modelo de dados](docs/03-modelo-de-dados.md) | Domínio, schema, atributos, CA/PA, saves |
| 04 | [Motor de partida](docs/04-motor-de-partida.md) | Simulação híbrida, calibração, orçamento de CPU |
| 05 | [Dados e jurídico](docs/05-dados-e-legal.md) | Fontes abertas, licenças, direitos de imagem, lojas |
| 06 | [UI/UX](docs/06-ui-ux.md) | Navegação desktop e mobile, densidade, acessibilidade |
| 07 | [Roadmap](docs/07-roadmap.md) | Marcos M0–M5, estimativas, equipe, custo |
| 08 | [Qualidade](docs/08-qualidade-e-testes.md) | Testes, golden masters, balanceamento, CI/CD |
| 09 | [Riscos](docs/09-riscos.md) | Registro de riscos com mitigação e gatilhos |
| 10 | [Referências: CM 03/04 e Elifoot 98](docs/10-referencias-motores.md) | O que aprendemos (e o que evitamos) dos dois motores clássicos |
| — | [ADRs](docs/adr/) | Decisões arquiteturais registradas |

## Resumo executivo em 10 linhas

* **Não é** um port, um patch, um "reskin" nem um projeto que lê arquivos do jogo original.
  É uma **reimplementação limpa** (clean-room): nenhum byte, asset ou base de dados do
  produto de 2003 entra aqui. Veja [`docs/05`](docs/05-dados-e-legal.md).
* **Núcleo** de simulação em **Rust** — determinístico, sem GC, compila para Windows,
  Android, iOS e WASM. **UI** em **Flutter** — um código para desktop e mobile, com
  listas densas virtualizadas e a visão 2D do campo em canvas próprio.
* **Determinismo é requisito de produto**, não detalhe: mesma seed + mesmos comandos =
  mesmo campeonato, byte a byte. É o que torna testável um sistema com 250 mil jogadores.
* **Dados** vêm de fontes abertas (openfootball/Wikidata, domínio público/CC0) e de
  geração procedural, com *data packs* da comunidade carregados pelo usuário.
* **Licença recomendada:** MPL-2.0 no código (compatível com App Store, ao contrário da
  GPL) e CC0/ODbL nos dados. Justificativa em [`docs/adr/0003`](docs/adr/0003-licenciamento.md).
* **Esforço estimado:** 14–18 meses até 1.0 com um time de 3–5 pessoas; ~30 meses em
  regime de hobby. O detalhamento por marco está em [`docs/07`](docs/07-roadmap.md).
* **Maior risco não-técnico:** direitos de personalidade e marcas sobre nomes reais de
  jogadores e clubes. Maior risco técnico: **balanceamento** do motor de partida — não a
  sua implementação.

## Estrutura do repositório

```
cm0304_open/
├─ core/                  # workspace Rust — núcleo determinístico headless
│  ├─ domain/              # ✅ ids, ponto fixo, calendário, RNG determinístico, atributos
│  ├─ pack/                # ✅ carregador + validador de data pack (nações, competições, clubes, jogadores)
│  ├─ engine/              # ✅ motor de partida v0.5 (força → chutes, finalização vs. goleiro → gol, docs/04 §7)
│  ├─ rules/               # ✅ calendário round-robin + chaveamento de mata-mata + tabela com
│  │                       #    desempate (docs/03 §7)
│  ├─ world/               # ✅ loop de temporada: calendário → partidas → tabela → acesso/queda
│  │                       #    (força de clube a partir do elenco de verdade, com fallback sintético)
│  │                       #    + progressão por idade (docs/03 §5.1) + lesões simples + mercado
│  │                       #    automático + copa (mata-mata de fase única, docs/01 RF-MU-03) +
│  │                       #    condição física por temporada (docs/01 RF-JG-07) + suspensões
│  │                       #    por expulsão (liga simulada rodada a rodada, docs/01 RF-JG-09) +
│  │                       #    folha salarial (M2, docs/01 RF-CL-02)
│  ├─ app/                 # ✅ dispatch/query (GameSession) — a fronteira que a ponte vai expor
│  │                       #    save/load por replay determinístico (docs/03 §8.1)
│  ├─ persist/             # ✅ save binário versionado — checksum, escrita atômica, sem blocos ainda
│  ├─ ai/                  # ✅ escalação automática (11 titulares por posição/CA) +
│  │                       #    primeira IA de mercado (docs/01 §2.4) — negociação/contratos ainda faltam
│  └─ cli/                 # ✅ managerfc-cli — version, pack validate, bench, calibrate, play, save,
│                          #    load, golden record/verify
├─ packs/core/             # ✅ pack de exemplo (1 país, 2 divisões, 16 clubes, 256 jogadores fictícios)
├─ app/                    # 🚧 interface Flutter (não confundir com core/app, acima) — esqueleto de
│                          #    fonte, sem SDK Flutter neste ambiente para gerar/verificar; ver app/README.md
├─ .github/workflows/      # ✅ CI do núcleo Rust (fmt, clippy, test, deny, e o portão do M0/M1 de verdade)
└─ docs/                   # ✅ escopo completo (00–10) + ADRs
```

## Como rodar o núcleo agora

```bash
cd core
cargo test --workspace              # 283 testes (inclui property tests e o pack de exemplo) — docs/08 §2
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p managerfc-cli -- version
cargo run -p managerfc-cli -- pack validate ../packs/core/example-two-tier
cargo run --release -p managerfc-cli -- bench     # RNF-01: ≤ 1,5 ms/partida em desktop
cargo run --release -p managerfc-cli -- play --seasons 3   # dispatch/query via app::GameSession
cargo run --release -p managerfc-cli -- calibrate --seasons 200   # gols/partida, vitórias, finalizações, conversão
cargo run --release -p managerfc-cli -- calibrate --seasons 50 --check   # falha se sair da tolerância (portão de CI)
cargo run --release -p managerfc-cli -- save --seasons 3 minha-carreira.cm0304save   # persist
cargo run --release -p managerfc-cli -- load minha-carreira.cm0304save              # replay do save
cargo run --release -p managerfc-cli -- golden verify --seed 42 --expect tests/golden/s42.json  # docs/08 §3
cargo run -p managerfc-cli -- --help
```

A interface Flutter ainda não tem SDK instalado/verificado neste repositório — ver
[`app/README.md`](app/README.md) para os próximos passos.

## Licença

Ainda não definida formalmente (ver ADR 0003). A recomendação vigente é **MPL-2.0** para
código e **CC0-1.0 / ODbL** para dados. Nenhum arquivo deste repositório deriva de
software ou base de dados de terceiros protegidos.

*Championship Manager* é marca do seu detentor atual (Embracer Group, via aquisição da
Eidos em 2022). Este projeto **não é afiliado, endossado ou licenciado** por ele, não
utiliza sua marca no produto final e não distribui nenhum de seus conteúdos.
