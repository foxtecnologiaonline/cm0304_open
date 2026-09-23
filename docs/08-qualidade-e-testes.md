# 08 — Qualidade, testes e balanceamento

> Em um jogo de simulação, o bug mais caro não é o que trava — é o que **desbalanceia sem
> travar**. Ele passa na revisão, passa no teste unitário e só aparece na temporada 8 de
> um jogador. A estratégia abaixo existe para pegar essa classe de defeito.

---

## 1. Pirâmide de testes

| Nível | O que cobre | Ferramenta | Quando roda |
|---|---|---|---|
| Unitário | Funções puras: cálculo de CA, valor de mercado, desempate de tabela | `cargo test` | todo commit |
| Propriedade | Invariantes com entradas geradas | `proptest` | todo commit |
| Golden master | Resultado congelado de simulações de referência | CLI + hash | todo PR |
| Integração | Temporada completa, transferências, progressão | CLI headless | todo PR |
| Calibração | Métricas estatísticas do motor | `calibrate` + tolerâncias | noturno + release |
| Longevidade | 20 temporadas simuladas | job noturno | noturno |
| Performance | Orçamentos de RNF | `criterion` + bench mobile | todo PR (desktop), noturno (mobile) |
| UI | Widgets e fluxos principais | `flutter test` + golden de widget | todo PR |
| Ponta a ponta | Nova carreira → temporada → save → recarregar | integração automatizada | noturno |
| Exploratório | Sessões humanas guiadas por roteiro | pessoas | por marco |

---

## 2. Invariantes (testes de propriedade)

Cada um é um teste `proptest` com milhares de casos gerados:

| # | Invariante |
|---|---|
| I1 | Soma ponderada de atributos ⇒ CA dentro de ±1 do valor armazenado, para qualquer jogador |
| I2 | CA ≤ PA, sempre, em qualquer ponto da carreira |
| I3 | Nenhum atributo fora de 1..=20; nenhuma condição fora de 0..=100 |
| I4 | Toda partida termina com 11 ≥ jogadores em campo ≥ 7 (regra de abandono) e ≤ 5 substituições |
| I5 | Soma de pontos da tabela = 3×vitórias + empates, para toda competição, toda rodada |
| I6 | Todo clube joga exatamente o número previsto de partidas na temporada |
| I7 | Dinheiro é conservado: toda transferência debita exatamente o que credita |
| I8 | Nenhum jogador aparece em dois clubes na mesma data |
| I9 | Jogador suspenso ou lesionado nunca entra em campo |
| I10 | Valor percebido (névoa de guerra) nunca vaza o valor real quando `precision < 100` |
| I11 | Carregar e salvar um save produz bytes idênticos (round-trip) |
| I12 | Log de comandos reproduzido do zero gera o mesmo hash de estado |

---

## 3. Golden masters

O mecanismo central de proteção contra regressão de simulação.

```bash
# gera a referência (só com bump intencional de sim_version)
managerfc-cli golden record --seed 42 --seasons 3 --pack packs/core --out tests/golden/s42.json

# verifica (roda em todo PR, nas 3 plataformas)
managerfc-cli golden verify --seed 42 --pack packs/core --expect tests/golden/s42.json
```

O arquivo guarda: hash de estado por dia, tabelas finais, artilheiros, transferências
principais e distribuição de CA. Falha de verificação tem **duas leituras possíveis**:

1. **Regressão** — corrigir o código; ou
2. **Mudança intencional de balanceamento** — então exige, no mesmo PR: bump de
   `sim_version`, nova gravação do golden, e uma nota explicando o efeito esperado.

Nunca se regrava um golden "para fazer a CI passar". Essa regra vai no `CONTRIBUTING.md`
e é verificada na revisão.

---

## 4. Calibração

Job noturno roda `calibrate --seasons 20 --seeds 20` e compara com os alvos de
[`04`](04-motor-de-partida.md#41-alvos-futebol-europeu-de-primeira-divisão-médias-recentes).
Resultado publicado como relatório (CSV + gráficos) e histórico versionado — a **tendência**
importa tanto quanto o valor absoluto.

Painéis mínimos do relatório:

* Gols/jogo, distribuição de placares, vitórias de mando, empates
* Finalizações, conversão, posse, cartões, lesões
* Correlação CA × pontos (a métrica de honestidade do motor)
* Distribuição de CA do mundo por temporada (detecta inflação/colapso)
* Inflação de valores de mercado ao longo de 20 temporadas
* Idade média dos titulares e dos artilheiros por liga (detecta regens quebrados)

---

## 5. Testes anti-exploit

Rodam em CI (nightly, e obrigatórios antes de release):

| Teste | Critério de falha |
|---|---|
| **Torneio de táticas** — ~40 táticas, elencos idênticos, round-robin duplo | qualquer tática > 62% de aproveitamento |
| **Escalação absurda** — goleiro na ponta, 3 fora de posição, 5 atacantes | vence a escalação equilibrada em série estatisticamente significativa |
| **Arbitragem de mercado** — bot compra e revende por 5 temporadas | lucro médio por operação > 30% do valor de mercado |
| **Fábrica de jovens** — clube pequeno só com juniores | título de 1ª divisão em ≤ 4 temporadas |
| **Bug de contrato** — renovações e cláusulas em massa | qualquer estado inconsistente ou dinheiro criado |

Cada teste que pega um exploit vira caso permanente da suíte — a suíte é memória
institucional do balanceamento.

---

## 6. Performance

```bash
managerfc-cli bench --budget docs/budgets.toml   # falha se ultrapassar RNF-01..07
```

* Benchmarks de desktop em todo PR; **benchmarks de mobile** em *device farm* ou aparelho
  dedicado, no ciclo noturno (RNF-02/03 no mobile são o orçamento que de fato restringe).
* Regressão > 10% em qualquer métrica falha a build e exige análise antes do merge.
* Perfil de memória (pico e residente) medido junto — RNF-08 é fácil de violar
  silenciosamente ao adicionar índices.

---

## 7. Testes de UI

* **Golden de widget** para os componentes de dados (`DataTable`, `AttributeGrid`,
  `FormationPitch`) em tema claro/escuro e em 3 densidades.
* Fluxos automatizados: nova carreira, escalar, jogar partida, aceitar oferta, salvar.
* Verificação de acessibilidade automatizada (contraste, rótulos semânticos, alvos de toque).
* Teste em aparelho real obrigatório antes de cada release: 1 Android de linha média,
  1 Android topo, 1 iPhone antigo suportado, 1 PC de especificação mínima.

---

## 8. CI/CD

```yaml
# esboço do pipeline (GitHub Actions)
pr:
  - fmt + clippy -D warnings + cargo-deny (licenças, RGPD de dependências)
  - cargo test (unit + proptest)
  - golden verify         [ubuntu, windows, macos → alvos x86-64 e ARM64]
  - cross-platform state-hash diff     # a checagem que protege o determinismo
  - cargo bench --budget
  - flutter analyze + flutter test
  - build: windows-zip, android-apk, ios-simulator
nightly:
  - calibrate --seasons 20 --seeds 20  → publica relatório
  - longevity 20 temporadas + verificação de invariantes
  - anti-exploit suite
  - benchmark em aparelho mobile real
release:
  - tudo acima + MSIX/AAB/IPA assinados + notas geradas + checklist jurídico
```

---

## 9. Definição de pronto (DoD)

Uma tarefa só está pronta quando:

- [ ] Testes unitários e, se toca simulação, teste de propriedade ou golden
- [ ] Nenhum orçamento de performance regredido além de 10%
- [ ] Documentação de domínio atualizada no mesmo PR (RNF-26)
- [ ] Nenhuma regra de jogo adicionada em código quando poderia ser dado (RNF-23)
- [ ] Nenhum `f32`/`f64` em caminho que afeta estado (verificado por lint) (RNF-12)
- [ ] Nenhum texto fixo em código fora do arquivo de localização
- [ ] Funciona no aparelho mobile de referência, não só no desktop

---

## 10. Gestão de defeitos

| Severidade | Definição | Prazo |
|---|---|---|
| **S1** | Corrompe save, trava, ou quebra determinismo | corrigir antes de qualquer outra coisa |
| **S2** | Regra de jogo errada com efeito em carreira (transferência, progressão, tabela) | no ciclo atual |
| **S3** | Balanceamento fora das tolerâncias | agrupado no próximo ciclo de calibração |
| **S4** | UI/UX, texto, cosmético | backlog priorizado |

Todo bug de simulação deve chegar com **seed + log de comandos**: com determinismo, isso
é reprodução garantida em um comando — e é o maior retorno prático de toda a disciplina
descrita em [`02`](02-arquitetura.md#5-determinismo).
