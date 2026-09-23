# 00 — Escopo mestre

**Documento de referência.** Qualquer discussão de "isso está no escopo?" se resolve aqui.
Mudanças neste arquivo exigem revisão explícita (é o contrato do projeto consigo mesmo).

---

## 1. Visão

> Recriar a *sensação* do Championship Manager 03/04 — profundidade absurda de base de
> dados, ritmo rápido, decisões de texto, visão 2D top-down da partida — em um produto
> aberto, moderno e portátil, que rode igualmente bem em um PC com Windows 11 e em um
> celular Android de linha média, com o mesmo save.

Três frases que definem o produto:

1. **É um jogo de leitura e decisão, não de espetáculo.** A tela principal é uma lista.
   O prazer vem de encontrar o jogador de 17 anos antes de todo mundo.
2. **O mundo continua sem você.** 40+ países simulados de forma crível, transferências,
   evolução e declínio de jogadores acontecendo em segundo plano.
3. **Um clique para continuar.** Loop de jogo rápido: *Continuar → notícias → decisão →
   Continuar*. Uma temporada inteira deve caber em um trajeto de metrô.

## 2. Por que existe

| Motivação | Detalhe |
|---|---|
| O original não roda bem hoje | Executável 32-bit de 2003, dependente de patches e camadas de compatibilidade; nada oficial para mobile. |
| A comunidade mantém o jogo vivo há 20+ anos | Bases de dados atualizadas, facepacks e patches continuam sendo publicados — existe demanda comprovada. |
| Os manager games modernos foram em outra direção | Mais simulação visual 3D, mais ciclos anuais pagos; o nicho "texto denso, rápido, moddável" está descoberto. |
| Nenhum projeto aberto ocupa esse espaço | Os projetos abertos existentes (ex.: *OpenFootManager*, *Bygfoot*) ou são de escopo menor, ou não miram desktop+mobile com paridade, ou estão em estágio inicial. |

## 3. Objetivos (o que é sucesso)

**Produto**

* O1 — Jogar uma temporada completa, de contratação a demissão, em qualquer das três
  plataformas-alvo, com o **mesmo arquivo de save**.
* O2 — Base inicial com **≥ 40 países**, **≥ 100 divisões jogáveis** e
  **≥ 200 mil jogadores** — paridade de escala com o original.
* O3 — **Visão 2D top-down** da partida com comentário sincronizado, além de modos texto
  e resultado instantâneo.
* O4 — **Editor pré-jogo** e formato de *data pack* aberto: a comunidade cria ligas,
  regras e bases sem recompilar o jogo.
* O5 — **Offline por padrão**, sem conta obrigatória, sem telemetria silenciosa, sem
  monetização no jogo base.

**Engenharia**

* O6 — **Determinismo total**: mesma seed + mesma sequência de comandos ⇒ mesmo estado
  final, em todas as plataformas (ver [ADR 0002](adr/0002-determinismo.md)).
* O7 — Núcleo simulável **headless** via CLI: uma temporada de 40 países em
  **< 3 minutos** em desktop, sem abrir a UI.
* O8 — **Calibração verificada** contra estatísticas reais (gols/jogo, vantagem de
  mando, distribuição de placares), com tolerâncias em CI.

## 4. Não-objetivos (o que explicitamente ficará de fora)

Esta lista vale tanto quanto a de objetivos. Um projeto desse porte morre por adição,
não por subtração.

| Não faremos | Por quê |
|---|---|
| **Ler, converter ou redistribuir arquivos do CM 03/04** (`.dat`, `.edt`, `.ddt`, saves) | Obra protegida de terceiros. Contamina o projeto inteiro e inviabiliza qualquer distribuição. Ver [`05`](05-dados-e-legal.md). |
| **Compatibilidade de save com o jogo original** | Consequência direta do item acima. |
| **Motor 3D / partida renderizada em 3D** | Custo altíssimo, benefício nulo para a proposta. O 2D top-down é a identidade do jogo. |
| **Multiplayer online em rede no 1.0** | Multiplica a complexidade (sincronia, anti-cheat, servidores, custo recorrente). Reavaliar pós-1.0; a arquitetura determinística já deixa a porta aberta (ver [`02`](02-arquitetura.md#94-hot-seat-e-o-caminho-para-multiplayer)). |
| **Assets oficiais**: escudos, kits reais, fotos de jogadores, hinos | Direitos de marca e imagem. Usaremos geração procedural + *packs* do usuário. |
| **Monetização, anúncios, loot boxes, assinatura** | Contraria a proposta e contamina decisões de design. |
| **Web / console / macOS / Linux no 1.0** | Não são alvo. macOS e Linux saem quase de graça da stack escolhida e serão builds "best effort" não suportados; web fica para depois (WASM já é possível pelo núcleo em Rust). |
| **IA generativa (LLM) para notícias/comentário no 1.0** | Custo, latência, imprevisibilidade e quebra de determinismo. Pode virar módulo opcional pós-1.0. |
| **Simulação física realista da bola** | O motor é probabilístico com *veneer* espacial; ver [`04`](04-motor-de-partida.md). |

## 5. Público-alvo

| Persona | Necessidade dominante | Impacto em decisões |
|---|---|---|
| **O veterano nostálgico (35–50 anos)** | Quer o ritmo e a densidade de 2003, não uma reinvenção | Atalhos de teclado, telas densas, ausência de tutorial obrigatório |
| **O jogador de metrô (mobile)** | Sessões de 5–15 min, retomada instantânea | Autosave agressivo, loop curto, UI de uma mão |
| **O modder / pesquisador de dados** | Criar ligas, editar bases, publicar packs | Formato de dados aberto e documentado, editor, CLI |
| **O contribuidor open source** | Entender e mudar o código em um fim de semana | Núcleo headless testável, docs, seeds determinísticas |

## 6. Escopo funcional — visão de alto nível

Detalhamento completo em [`01-requisitos.md`](01-requisitos.md). Resumo por área:

```
┌─ MUNDO ──────────────┬─ CLUBE ───────────────┬─ PARTIDA ──────────────┐
│ Países, ligas, copas │ Elenco, contratos     │ Escalação, táticas     │
│ Calendário global    │ Finanças, estádio     │ Motor 2D + comentário  │
│ Reputação, rankings  │ Treino, comissão      │ Substituições, gritos  │
│ Transferências (IA)  │ Juniores, regens      │ Relatório, estatísticas│
├─ JOGADOR ────────────┼─ CARREIRA ────────────┼─ META ─────────────────┤
│ Atributos 1–20       │ Empregos, entrevistas │ Editor pré-jogo        │
│ CA/PA, evolução      │ Diretoria, metas      │ Data packs, modding    │
│ Moral, condição      │ Imprensa, reputação   │ Saves, nuvem opcional  │
│ Lesões, suspensões   │ Demissão, ofertas     │ Localização PT/EN/ES   │
└──────────────────────┴───────────────────────┴────────────────────────┘
```

## 7. MVP — a menor coisa que prova a tese

O MVP **não** é o jogo. É o menor artefato que responde: *"o loop é divertido e a
arquitetura aguenta?"*. Entrega-se ao fim do **M2** (ver [`07`](07-roadmap.md)).

**Dentro do MVP**

* 1 país, 2 divisões, ~48 clubes, ~1.500 jogadores (base gerada + dados abertos).
* Temporada completa: calendário, rodadas, tabela, promoção/rebaixamento, copa nacional.
* Elenco: escalação, atributos, moral, condição, lesões simples, suspensões.
* Táticas: formação, mentalidade, marcação, instruções por jogador (subconjunto).
* Motor de partida em **modo texto + placar ao vivo** (o 2D entra no M3).
* Transferências: ofertas da IA, negociação simples, contratos.
* Treino: regime por jogador com efeito mensurável na evolução.
* UI: Windows **e** Android a partir do mesmo código, com o mesmo save.
* Save/load, autosave, 3 slots.

**Fora do MVP** (entra em M3+): visão 2D, scouting com névoa de guerra, juniores/regens,
múltiplos países, competições continentais, imprensa, editor, mercado de empréstimos,
comissão técnica, finanças detalhadas, localização.

**Critério de aprovação do MVP** — os quatro precisam passar:

1. 10 testadores jogam ≥ 1 temporada; ≥ 7 pedem espontaneamente para continuar jogando.
2. Uma temporada headless roda em < 5 s (desktop) e < 20 s (celular de linha média).
3. Métricas do motor dentro das tolerâncias de [`08`](08-qualidade-e-testes.md#4-calibração).
4. Save gerado no Windows abre no Android e continua idêntico (teste automatizado).

Se 1 falhar, o problema é de *design* — nenhuma quantidade de features corrige.
Se 2–4 falharem, o problema é de *arquitetura* — e o custo de corrigir só cresce.

## 8. Fases (resumo)

| Marco | Nome | Entrega | Duração |
|---|---|---|---|
| **M0** | Fundação | Repo, CI, ADRs, esqueleto core+UI, 1 liga carregando | 4–6 sem |
| **M1** | Kick-off headless | Temporada completa simulada por CLI, sem UI | 8–10 sem |
| **M2** | **MVP jogável** | Vertical slice: 1 país, UI Windows+Android, save cross-platform | 10–12 sem |
| **M3** | Alpha | Visão 2D, táticas completas, treino, scouting, juniores | 12–14 sem |
| **M4** | Beta | 40+ países, continentais, editor, balanceamento, iOS | 12–14 sem |
| **M5** | 1.0 | Modding, localização, lojas, acessibilidade, polimento | 10–12 sem |

Detalhes, dependências, equipe e custo em [`07-roadmap.md`](07-roadmap.md).

## 9. Premissas

* P1 — Time pequeno (3–5 pessoas) ou comunidade equivalente; **não** há orçamento de
  licenciamento de dados (FIFPro e similares estão fora de alcance).
* P2 — Distribuição inicial: **itch.io + GitHub Releases** (Windows/Android APK).
  Lojas oficiais (Play/App Store/Steam) só após revisão jurídica — ver [`05`](05-dados-e-legal.md).
* P3 — Não há compromisso com ciclo anual de versões; o jogo é atualizado por *data packs*.
* P4 — O projeto assume que **nenhum** colaborador tenha acesso a código-fonte,
  documentação interna ou assets do jogo original (regra de clean-room).

## 10. Restrições

* R1 — Mobile de linha média (≈ 4 GB RAM, big.LITTLE) é o piso de performance. Todo
  orçamento de CPU/memória é definido por ele, não pelo desktop.
* R2 — iOS impõe **JIT proibido**, revisão de loja e custo anual de conta de
  desenvolvedor; também é incompatível na prática com licenças GPL (ver [ADR 0003](adr/0003-licenciamento.md)).
* R3 — Sem servidores: qualquer funcionalidade "online" precisa ser opcional e degradar
  para 100% offline.
* R4 — Tamanho do app nas lojas: alvo ≤ 150 MB base; bases grandes baixadas como pack
  sob demanda.

## 11. Glossário

| Termo | Significado |
|---|---|
| **CA / PA** | *Current / Potential Ability* — habilidade atual e potencial, escala interna 0–200, oculta do jogador |
| **Atributo** | Valor visível 1–20 (finalização, passe, ritmo…) |
| **Regen / newgen** | Jogador gerado proceduralmente que substitui um aposentado |
| **Data pack** | Pacote aberto (dados + regras) carregável sem recompilar |
| **Headless** | Execução do núcleo sem interface, via CLI — base de testes e balanceamento |
| **Golden master** | Resultado de referência congelado, comparado em CI para detectar regressão de simulação |
| **Tick** | Menor unidade de tempo do motor de partida (alvo: 1 s de jogo simulado) |
