# 01 — Requisitos

Cada requisito tem **ID estável** (`RF-xx-nn` funcional, `RNF-nn` não-funcional), uma
**prioridade MoSCoW** e o **marco** em que entra. IDs nunca são reaproveitados.

Prioridade: **M** = Must (1.0 não existe sem), **S** = Should, **C** = Could, **W** = Won't (registrado para não voltar à pauta).

---

## 1. Épicos

| Épico | Código | Descrição | Marco principal |
|---|---|---|---|
| Mundo e competições | `MU` | Países, ligas, copas, calendário, regras | M1 |
| Clube e elenco | `CL` | Plantel, contratos, finanças, infraestrutura | M1–M2 |
| Jogador e progressão | `JG` | Atributos, CA/PA, evolução, lesões, moral | M1–M3 |
| Transferências | `TR` | Mercado, negociação, empréstimos, IA | M2–M4 |
| Táticas e partida | `PA` | Escalação, instruções, motor, 2D | M2–M3 |
| Treino e comissão | `TC` | Regimes, staff, desenvolvimento | M3 |
| Observação (scouting) | `SC` | Relatórios, névoa de guerra, rede de olheiros | M3 |
| Carreira e meta-jogo | `CA` | Diretoria, imprensa, empregos, metas | M3–M4 |
| Dados e modding | `DM` | Data packs, editor, import/export | M4–M5 |
| Plataforma | `PL` | Saves, UI, localização, acessibilidade, lojas | M2–M5 |

---

## 2. Requisitos funcionais

### 2.1 Mundo e competições (`MU`)

| ID | Requisito | Pri | Marco |
|---|---|---|---|
| RF-MU-01 | Estrutura hierárquica país → divisões → clubes, com promoção/rebaixamento configurável | M | M1 |
| RF-MU-02 | Geração de calendário com round-robin, datas FIFA, feriados e conflitos de estádio | M | M1 |
| RF-MU-03 | Copas nacionais: mata-mata, replays, jogos de ida e volta, gol fora (configurável) | M | M1 |
| RF-MU-04 | Regras de competição **dirigidas por dados** (JSON/DSL), não por código | M | M1 |
| RF-MU-05 | Classificação com critérios de desempate configuráveis por competição | M | M1 |
| RF-MU-06 | Competições continentais com vagas, fases de grupo e coeficientes | M | M4 |
| RF-MU-07 | Seleções nacionais, convocações, eliminatórias e torneios | S | M4 |
| RF-MU-08 | Reputação de país/clube influenciando atratividade, receita e IA | M | M2 |
| RF-MU-09 | Ligas "em segundo plano" (simulação simplificada) para países não carregados em detalhe | M | M2 |
| RF-MU-10 | Regras de elegibilidade: estrangeiros, cria local, limite de inscritos, idade | S | M4 |
| RF-MU-11 | Arbitragem com perfis (rigor, tendência a cartões) afetando a partida | C | M4 |

### 2.2 Clube e elenco (`CL`)

| ID | Requisito | Pri | Marco |
|---|---|---|---|
| RF-CL-01 | Plantel com posições, números, status (principal/rotação/reserva) | M | M1 |
| RF-CL-02 | Contratos: salário, duração, luvas, cláusulas (rescisão, gols, aparições) | M | M2 |
| RF-CL-03 | Finanças: receita (bilheteria, TV, prêmios, patrocínio), despesa, saldo, dívida | M | M2 |
| RF-CL-04 | Orçamento de transferências e de salários, com negociação junto à diretoria | M | M3 |
| RF-CL-05 | Estádio, capacidade, obras, centro de treinamento e categorias de base (níveis) | S | M4 |
| RF-CL-06 | Comissão técnica: auxiliar, preparador, fisioterapeuta, olheiros, com atributos | S | M3 |
| RF-CL-07 | Time B / sub-19 com jogos próprios e promoção de atletas | S | M4 |
| RF-CL-08 | Histórico do clube: títulos, recordes, ídolos, sequências | C | M5 |

### 2.3 Jogador e progressão (`JG`)

| ID | Requisito | Pri | Marco |
|---|---|---|---|
| RF-JG-01 | Atributos visíveis 1–20 em técnica, mental e física | M | M1 |
| RF-JG-02 | Atributos ocultos (consistência, jogo importante, sujeira, lealdade, adaptação, propensão a lesão, ambição, profissionalismo) | M | M2 |
| RF-JG-03 | CA/PA internos 0–200 com custo ponderado por posição (ver [`03`](03-modelo-de-dados.md#5-caa-pa-e-o-custo-de-atributos)) | M | M1 |
| RF-JG-04 | Familiaridade posicional 0–20 por posição, com aprendizado via treino | M | M2 |
| RF-JG-05 | Curva de evolução por idade, jogos, treino, moral e qualidade do clube | M | M2 |
| RF-JG-06 | Declínio e aposentadoria, com transição opcional para comissão técnica | S | M4 |
| RF-JG-07 | Moral, condição física, fadiga acumulada e frescor de jogo | M | M2 |
| RF-JG-08 | Lesões com tipo, gravidade, prazo de recuperação e risco de recaída | M | M3 |
| RF-JG-09 | Cartões, suspensões automáticas por competição | M | M2 |
| RF-JG-10 | Personalidade derivada de atributos ocultos, afetando reação a decisões | S | M4 |
| RF-JG-11 | Regens: geração procedural de juniores por país/clube, com nacionalidade, nome e potencial plausíveis | M | M3 |
| RF-JG-12 | Dupla nacionalidade e naturalização | C | M4 |

### 2.4 Transferências (`TR`)

| ID | Requisito | Pri | Marco |
|---|---|---|---|
| RF-TR-01 | Janelas de transferência por país, com regras de inscrição | M | M2 |
| RF-TR-02 | Avaliação de valor de mercado (CA, PA, idade, contrato, reputação, demanda) | M | M2 |
| RF-TR-03 | Negociação clube↔clube: valor, parcelas, bônus, percentual de revenda, troca | M | M2 |
| RF-TR-04 | Negociação clube↔jogador: salário, cláusulas, papel no elenco, agente | M | M2 |
| RF-TR-05 | IA de transferência: clubes identificam carência, orçam, disputam e desistem | M | M2 |
| RF-TR-06 | Empréstimos, com participação salarial, obrigação/opção de compra e cláusula de não-enfrentamento | S | M3 |
| RF-TR-07 | Jogadores livres, pré-contratos e fim de contrato | S | M3 |
| RF-TR-08 | Lista de dispensa/transferência, sondagens e propostas rejeitadas com memória | S | M3 |
| RF-TR-09 | Inflação de mercado coerente ao longo de 20+ temporadas | M | M4 |

### 2.5 Táticas e partida (`PA`)

| ID | Requisito | Pri | Marco |
|---|---|---|---|
| RF-PA-01 | Escalação com formação, posições, capitão, cobradores | M | M2 |
| RF-PA-02 | Instruções de equipe: mentalidade, ritmo, largura, linha defensiva, pressão, marcação | M | M2 |
| RF-PA-03 | Instruções por jogador: liberdade, avanço, cruzamento, desarme, marcação individual | M | M3 |
| RF-PA-04 | Motor de partida determinístico por tick (ver [`04`](04-motor-de-partida.md)) | M | M1 |
| RF-PA-05 | Três modos de visualização: 2D top-down, texto comentado, resultado instantâneo | M | M3 (2D) / M2 (texto) |
| RF-PA-06 | Substituições, mudanças táticas e instruções em tempo real, com pausa | M | M2 |
| RF-PA-07 | Estatísticas de partida: posse, finalizações, xG, passes, desarmes, notas | M | M2 |
| RF-PA-08 | Relatório pós-jogo, destaques e reação de imprensa | S | M4 |
| RF-PA-09 | Replay/salvar partida para rever (o original tinha; barato com determinismo) | C | M4 |
| RF-PA-10 | Bolas paradas: escanteios, faltas, pênaltis com cobradores definidos | S | M3 |
| RF-PA-11 | Condições: clima, gramado, altitude, público, influenciando o motor | C | M4 |

### 2.6 Treino e comissão (`TC`)

| ID | Requisito | Pri | Marco |
|---|---|---|---|
| RF-TC-01 | Regimes de treino por jogador (categorias: força, técnica, tática, finalização…) | M | M3 |
| RF-TC-02 | Intensidade com efeito real em fadiga e risco de lesão | M | M3 |
| RF-TC-03 | Delegação ao auxiliar técnico, com qualidade dependente do atributo do staff | S | M3 |
| RF-TC-04 | Relatórios de progresso e alerta de estagnação/regressão | S | M3 |
| RF-TC-05 | Treino individual focado em atributo/posição específica | S | M4 |

### 2.7 Observação (`SC`)

| ID | Requisito | Pri | Marco |
|---|---|---|---|
| RF-SC-01 | **Névoa de guerra**: atributos exibidos como faixa, com precisão crescente conforme conhecimento | M | M3 |
| RF-SC-02 | Designar olheiros para região/competição/jogador; relatórios com viés do olheiro | M | M3 |
| RF-SC-03 | Filtros de busca com muitos critérios e listas salvas | M | M3 |
| RF-SC-04 | Shortlist com acompanhamento e alertas (contrato acabando, queda de preço) | S | M4 |
| RF-SC-05 | Conhecimento do clube por país/liga, aumentando com presença e olheiros | S | M4 |

### 2.8 Carreira e meta-jogo (`CA`)

| ID | Requisito | Pri | Marco |
|---|---|---|---|
| RF-CA-01 | Perfil de técnico: nome, nacionalidade, experiência prévia, reputação | M | M2 |
| RF-CA-02 | Metas da diretoria por temporada e avaliação periódica de confiança | M | M3 |
| RF-CA-03 | Demissão e busca por novo emprego; candidaturas e convites | M | M3 |
| RF-CA-04 | Imprensa: entrevistas pré/pós-jogo com efeito em moral (conjunto pequeno e honesto de opções) | S | M4 |
| RF-CA-05 | Confiança do elenco, conversas individuais e gestão de insatisfação | S | M4 |
| RF-CA-06 | Seleção nacional como cargo paralelo | C | M5 |
| RF-CA-07 | Caixa de entrada unificada, filtrável, com "processar tudo" | M | M2 |

### 2.9 Dados e modding (`DM`)

> Nota de sequenciamento: o **carregador mínimo** (`pack` crate — nações,
> competições, clubes, ids densos, validação referencial) já existe desde o
> M0, porque `world`/`rules` precisam dele para ter o que quer que seja para
> processar (`docs/07-roadmap.md#m0--fundação-6-semanas`). O que os itens
> abaixo descrevem para o M4 é a **superfície pública e madura** de modding:
> formato versionado e documentado para terceiros, editor visual, múltiplos
> packs simultâneos com resolução de conflito. `RF-DM-04` (validador com
> mensagens úteis) já está parcialmente entregue pelo CLI
> (`managerfc-cli pack validate`); o que falta para M4 é a UI *dentro do
> jogo* e a validação de `people/*.json` e `rules/*.toml`, que ainda não
> existem.

| ID | Requisito | Pri | Marco |
|---|---|---|---|
| RF-DM-01 | Formato de **data pack** aberto e versionado (ver [`03`](03-modelo-de-dados.md#7-data-packs)) | M | M4 |
| RF-DM-02 | Editor pré-jogo: criar/editar clubes, jogadores, competições, regras | M | M4 |
| RF-DM-03 | Import/export CSV e JSON para edição em ferramentas externas | S | M4 |
| RF-DM-04 | Validador de pack com mensagens de erro úteis (CLI + no jogo) | M | M4 |
| RF-DM-05 | Múltiplos packs ativos com resolução de conflito determinística | S | M5 |
| RF-DM-06 | Packs de cosmético (escudos SVG, nomes, uniformes) separados dos de regra | S | M5 |
| RF-DM-07 | Geração procedural de mundo fictício completo ("new world") | C | M5 |

### 2.10 Plataforma (`PL`)

| ID | Requisito | Pri | Marco |
|---|---|---|---|
| RF-PL-01 | Save/load com slots, autosave e recuperação de crash | M | M2 |
| RF-PL-02 | Save **binário compatível entre plataformas** (mesmo arquivo no Windows e no Android) | M | M2 |
| RF-PL-03 | Migração de save entre versões do jogo (ou aviso claro de incompatibilidade) | M | M4 |
| RF-PL-04 | UI adaptativa: densa no desktop, uma-mão no celular, split-view no tablet | M | M2 |
| RF-PL-05 | Atalhos de teclado completos no desktop (navegação sem mouse) | S | M3 |
| RF-PL-06 | Localização PT-BR, EN, ES no 1.0; arquitetura pronta para mais | M | M5 |
| RF-PL-07 | Acessibilidade: escala de fonte, contraste, leitor de tela nas listas principais | S | M5 |
| RF-PL-08 | Exportação de save/estatísticas (compartilhar carreira) | C | M5 |
| RF-PL-09 | Sincronização opcional de save via armazenamento do próprio usuário (arquivo/nuvem pessoal) | C | M5 |

### 2.11 Registrado como Won't (`W`)

| ID | Item | Motivo |
|---|---|---|
| RF-W-01 | Importar dados/saves do CM 03/04 | Jurídico — ver [`05`](05-dados-e-legal.md) |
| RF-W-02 | Multiplayer online | Escopo e custo recorrente |
| RF-W-03 | Partida em 3D | Fora da identidade do produto |
| RF-W-04 | Compras dentro do app | Contraria a proposta |
| RF-W-05 | Narração por LLM no 1.0 | Determinismo, custo, latência |

---

## 3. Requisitos não-funcionais

Números são **orçamentos**, verificados em CI (ver [`08`](08-qualidade-e-testes.md)).
Referência mobile: aparelho de linha média com ~4 GB de RAM.

### 3.1 Performance

| ID | Requisito | Alvo desktop | Alvo mobile |
|---|---|---|---|
| RNF-01 | Simular 1 partida em modo instantâneo | ≤ 1,5 ms | ≤ 6 ms |
| RNF-02 | Processar 1 dia de calendário mundial (40 países) | ≤ 400 ms | ≤ 1,5 s |
| RNF-03 | Simular 1 temporada completa headless | ≤ 25 s | ≤ 90 s |
| RNF-04 | Carregar save de 250k jogadores | ≤ 2 s | ≤ 5 s |
| RNF-05 | Visão 2D estável | 60 fps | 30 fps (mín.) |
| RNF-06 | Resposta de navegação da UI (tela → tela) | ≤ 100 ms | ≤ 150 ms |
| RNF-07 | Filtro/busca em 250k jogadores | ≤ 250 ms | ≤ 600 ms |

### 3.2 Recursos

| ID | Requisito | Alvo |
|---|---|---|
| RNF-08 | Memória residente com base completa carregada | ≤ 1,2 GB desktop / ≤ 600 MB mobile |
| RNF-09 | Tamanho do save (base completa, 1 temporada) | ≤ 80 MB comprimido |
| RNF-10 | Tamanho do instalador base | ≤ 150 MB |
| RNF-11 | Consumo de bateria em 1 h de jogo (mobile) | ≤ 12% em aparelho de referência |

### 3.3 Qualidade de engenharia

| ID | Requisito |
|---|---|
| RNF-12 | **Determinismo**: mesma seed + mesmos comandos ⇒ mesmo hash de estado, em todas as plataformas e arquiteturas (x86-64 e ARM64) |
| RNF-13 | Núcleo executável headless sem nenhuma dependência de UI |
| RNF-14 | Cobertura de testes ≥ 80% no núcleo de simulação; regras de competição 100% |
| RNF-15 | Build reprodutível; CI cobrindo Windows, Android e iOS em todo PR |
| RNF-16 | Nenhuma chamada de rede obrigatória em tempo de execução |
| RNF-17 | Telemetria **opt-in**, anônima, desligada por padrão, documentada |
| RNF-18 | Logs locais com redação de dados pessoais; nenhum envio automático |

### 3.4 Portabilidade e compatibilidade

| ID | Requisito |
|---|---|
| RNF-19 | Windows 10 21H2+ e Windows 11 (x64 e ARM64), sem privilégios de administrador |
| RNF-20 | Android 9 (API 28)+ em ARM64; iOS 15+ |
| RNF-21 | Resoluções de 360×640 (celular) a 3840×2160 (desktop), com escala de DPI |
| RNF-22 | Funcionamento 100% offline, inclusive na primeira execução |

### 3.5 Manutenibilidade e governança

| ID | Requisito |
|---|---|
| RNF-23 | Toda regra de competição, econômica ou de progressão exposta como dado editável, não constante de código |
| RNF-24 | ADR obrigatório para qualquer decisão que afete determinismo, formato de save ou stack |
| RNF-25 | Formato de save versionado com migração automática ou erro explícito |
| RNF-26 | Documentação de domínio mantida junto ao código (docs como parte do PR) |

---

## 4. Backlog priorizado (primeiras 3 ondas)

**Onda 1 — M0/M1 (fundação e mundo simulável)**
1. Esqueleto do núcleo Rust + CLI headless + RNG determinístico semeado
2. Modelo de dados de jogador/clube/competição + carregador de data pack (1 país)
3. Motor de partida v0 (probabilístico puro, sem eventos espaciais)
4. Calendário, tabela, promoção/rebaixamento, copa nacional
5. Loop de temporada + relatório de calibração em CSV
6. CI multiplataforma + golden master do motor

**Onda 2 — M2 (MVP jogável)**
7. Ponte Rust↔Flutter e shell de UI (navegação, listas virtualizadas)
8. Telas: elenco, jogador, tabela, calendário, caixa de entrada, escalação
9. Motor v1 com eventos por tick e comentário de texto
10. Transferências (IA + negociação) e contratos
11. Save binário cross-platform + autosave
12. Build Windows (MSIX/zip) e Android (APK) no CI

**Onda 3 — M3 (alpha)**
13. Visão 2D top-down com interpolação de posições
14. Táticas completas (instruções por jogador, bolas paradas)
15. Treino + comissão técnica
16. Scouting com névoa de guerra
17. Lesões detalhadas, regens, evolução calibrada em 10 temporadas
18. Diretoria, metas, demissão e busca de emprego
