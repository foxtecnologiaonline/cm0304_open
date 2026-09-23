# 09 — Registro de riscos

Escala: **P** = probabilidade (1–5), **I** = impacto (1–5), **E** = exposição (P×I).
Revisão obrigatória ao fim de cada marco.

---

## 1. Top 10 por exposição

| # | Risco | P | I | E | Mitigação | Gatilho de ação |
|---|---|---|---|---|---|---|
| R1 | **Balanceamento do motor nunca converge** — o jogo simula, mas não é divertido nem crível | 4 | 5 | **20** | Calibração automatizada desde o M1; alvos numéricos explícitos; torneio anti-exploit em CI; playtests a cada marco | Duas rodadas de calibração seguidas fora da tolerância ⇒ congelar features e abrir força-tarefa |
| R2 | **Escopo infla e o projeto morre de exaustão** | 5 | 4 | **20** | Lista de não-objetivos vinculante; ordem de corte pré-acordada ([`07`](07-roadmap.md#7-como-o-escopo-é-cortado-sob-pressão)); portões por marco | Marco estourar prazo em > 30% ⇒ aplicar corte, não estender prazo |
| R3 | **Notificação jurídica sobre nomes reais/marca** | 2 | 5 | **10** | Modo genérico como padrão de loja; conteúdo real só em pack separado; marca própria; revisão jurídica antes do M5 ([`05`](05-dados-e-legal.md)) | Qualquer contato formal ⇒ acionar plano de resposta §7 de [`05`](05-dados-e-legal.md) |
| R4 | **Base de dados inicial ruim** — atributos sem credibilidade afundam a percepção do jogo | 4 | 4 | **16** | Pipeline de ratings auditável; curadoria comunitária com histórico; modo fictício plausível como alternativa | Testadores relatarem "os jogadores estão errados" em > 30% das sessões ⇒ priorizar pipeline |
| R5 | **Performance no mobile não fecha com base completa** | 3 | 4 | **12** | Orçamentos desde o M0; LOD por competição; SoA + colunas frias; benchmark em aparelho real no noturno | Estouro de RNF-02/08 por 2 ciclos ⇒ reduzir países padrão no mobile e carregar sob demanda |
| R6 | **Determinismo cross-platform quebra tarde** | 2 | 5 | **10** | Ponto fixo obrigatório; lint contra float; hash cruzado em CI desde o M0 | Qualquer divergência ⇒ S1, para a fila |
| R7 | **Time/comunidade perde tração** (fator de ônibus) | 4 | 3 | **12** | Núcleo headless e documentado; ADRs; rotação de revisão; tarefas de entrada bem marcadas | Um marco sem commits de uma área ⇒ redistribuir e reduzir escopo formalmente |
| R8 | **`flutter_rust_bridge` ou o ecossistema Flutter quebra o fluxo de build** | 3 | 3 | **9** | Superfície de FFI mínima e estável; versões fixadas; plano B (FFI manual / UI nativa) documentado | Duas atualizações seguidas exigindo > 1 semana ⇒ avaliar plano B no M4 |
| R9 | **Jogadores esperam "o CM 03/04 exatamente igual" e rejeitam o produto** | 4 | 2 | **8** | Comunicação honesta desde o primeiro dia: inspiração, não clone; sem promessa de compatibilidade; nome próprio | Discurso de comunidade derivar para "port" ⇒ corrigir publicamente |
| R10 | **Formato de save engessa a evolução** | 3 | 3 | **9** | Versionamento + migrações obrigatórias com fixtures desde o M2; blocos independentes | Duas migrações seguidas impossíveis ⇒ revisar formato antes do beta |

---

## 2. Riscos menores monitorados

| Risco | Nota |
|---|---|
| Rejeição em loja (Apple/Google) por conteúdo ou metadados | Mitigado pelo modo genérico e pela ficha de privacidade honesta; submeter cedo (M4) para descobrir problemas antes do 1.0 |
| Custos de CI (runners macOS) crescerem | Limitar builds iOS ao ciclo noturno e a releases |
| Tamanho do app acima do limite de loja | Base grande como pack sob demanda (RNF-10) |
| Contribuição contaminada (código/dados do original) | DCO + regra de clean-room no template de PR + revisão de PRs com dados binários |
| Tradução travar o lançamento | Localização começa no M2 (sem string fixa em código), não no M5 |
| Expectativa de multiplayer | Registrado como Won't (RF-W-02) desde o início; arquitetura não fecha a porta, mas o discurso não promete |

---

## 3. Riscos que aceitamos sem mitigação

| Risco aceito | Por quê |
|---|---|
| Não teremos escudos, fotos nem uniformes oficiais | Impossível sem licença; o público-alvo historicamente resolve isso com packs |
| O motor não será tão rico quanto o de um estúdio com 20 anos de iteração | Compensa-se em abertura, moddabilidade, velocidade e ausência de monetização |
| Não haverá ciclo anual de versões | O modelo é data pack + atualizações contínuas |
| macOS e Linux serão "best effort" | Saem quase de graça da stack, mas sem compromisso de suporte |

---

## 4. Indicadores de alerta precoce

Três métricas observadas a cada marco — qualquer uma no vermelho aciona revisão de escopo:

1. **Tempo de ciclo do PR** (abertura → merge). Acima de 5 dias = time sobrecarregado ou
   arquitetura acoplada demais.
2. **Taxa de golden master regravado por mês.** Alta = balanceamento instável (R1).
3. **Retenção em playtest** (quantos testadores pedem para continuar jogando). É o único
   indicador que mede a coisa que realmente importa.
