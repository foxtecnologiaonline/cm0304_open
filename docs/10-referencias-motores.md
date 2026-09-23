# 10 — Referências: os motores do CM 03/04 e do Elifoot 98

> Este documento **não** é engenharia reversa nossa, não contém código nem dado extraído
> de nenhum dos dois jogos, e nenhuma linha daqui é copiada para o produto (ver regra de
> clean-room em [`05-dados-e-legal.md`](05-dados-e-legal.md)). É leitura crítica de
> **material publicado publicamente** — reviews, guias de jogador, análises de
> engenharia reversa de terceiros e relatos históricos — usada como **estudo de caso**
> para as decisões de [`04-motor-de-partida.md`](04-motor-de-partida.md). A pergunta que
> este documento responde é sempre "o que isso ensina para o *nosso* motor?", nunca
> "como replicar isso?".

Dois motores, duas filosofias opostas, e — coincidentemente — o mesmo tipo de bug no
final: um vetor de força mal balanceado que uma tática degenerada consegue explorar.
É a lição central deste documento e a justificativa direta do
[torneio anti-exploit em CI](04-motor-de-partida.md#4-calibração) que já está no escopo.

---

## 1. Championship Manager: Season 03/04 (Sports Interactive / Eidos, 2003)

### 1.1 O que o motor é

CM 03/04 herdou do *Championship Manager 4* (2002) a **visão 2D top-down** da partida —
o elemento mais identificável da série e o que este projeto também adota como modo
principal de visualização (RF-PA-05). A crítica da época descreve o motor 2D como
"surpreendentemente eficaz em mostrar como e por que seu time está performando", com
sinalizações visuais de condição do gramado, bandeiras de impedimento e clima.

Por baixo da visão 2D, o jogo usava (como toda a linhagem Championship
Manager/Football Manager da época) um sistema de **atributos visíveis 1–20** por
jogador e um par de valores ocultos de habilidade — **Current Ability (CA)** e
**Potential Ability (PA)** — que a comunidade de pesquisa dessa série de jogos descreve
consistentemente da seguinte forma:

* **PA é fixo** desde a criação do jogador (0–99 na notação clássica da série; em
  edições posteriores, 0–200): não sobe nem desce, e é atribuído por curadoria humana no
  editor pré-jogo, não por fórmula.
* **CA pode subir e descer**, mas **nunca ultrapassa PA** — é o "teto genético" do
  jogador.
* CA é descrito pela comunidade como uma **soma ponderada dos atributos visíveis**,
  ajustada por posição — o mesmo princípio geral que adotamos em
  [`03-modelo-de-dados.md §5`](03-modelo-de-dados.md#5-ca-pa-e-o-custo-de-atributos),
  embora nossa fórmula e nossos pesos sejam obra própria, calibrados em dados abertos, e
  não uma tentativa de reproduzir a fórmula do jogo original (que nunca foi publicada
  oficialmente — o que existe é engenharia reversa de terceiros sobre versões da série).

### 1.2 O que aprendemos daqui (adotado)

| Lição | Onde entra no nosso design |
|---|---|
| Visão 2D top-down é a identidade do gênero — não precisa ser 3D para transmitir "estou vendo o jogo" | [`04 §2.4`](04-motor-de-partida.md#24-posições-para-o-2d) |
| CA/PA como par oculto dá progressão de carreira crível sem expor números "de RPG" ao jogador | [`03 §5`](03-modelo-de-dados.md#5-ca-pa-e-o-custo-de-atributos) |
| PA fixo na criação, CA como teto móvel, é suficiente para gerar 20+ temporadas de mundo plausível | [`03 §5.1`](03-modelo-de-dados.md#51-progressão-resumo-detalhe-de-calibração-em-08) |
| Atributos 1–20 são legíveis o bastante para o jogador decidir tática sem precisar de um manual | [`03 §3.1`](03-modelo-de-dados.md#31-atributos-visíveis-n_attr--36) |

### 1.3 O que aprendemos daqui (evitado deliberadamente)

**A tática "Diablo".** O bug tático mais famoso da série inteira apareceu justamente na
geração de motor do CM 03/04: um 4-1-3-2 largo, batizado pelo jogador que o divulgou
(*"El Rosso Diablo"*), construído em torno de um meio-campista central com liberdade
total e uma seta de avanço ("forward run") que ia até a linha do centroavante. Esse
jogador **nunca era marcado corretamente pela IA defensiva** e finalizava, com
tranquilidade, um gol atrás do outro da entrada da área. O caso ficou famoso a ponto de
ser citado como inspiração real de um técnico (José Mourinho) para o papel de box-to-box
que deu a Frank Lampard em Chelsea — ficção de jogo virando referência tática real.

Dois outros exploits recorrentes documentados por guias de jogadores da época:

* **Escanteio "near post"**: a combinação de cobrança fechada no primeiro pau + instrução
  individual "Attack Near Post" + "Challenge GK" era descrita como fonte quase
  automática de gols — um caso de **instrução específica demais** encontrando um ponto
  cego da IA de marcação em bola parada.
* **Pontas velozes**: alas com ritmo/aceleração/drible ≥ 17 batiam sistematicamente
  qualquer lateral adversário, porque a comparação de atributos de duelo não tinha
  contrapeso suficiente (cobertura, ajuda posicional) para times sem essa característica
  específica.

**Por que isso importa para nós:** os três casos têm a **mesma raiz estrutural** — uma
combinação de instrução tática explora um vetor de força que o motor calcula de forma
isolada, sem o resto do sistema "perceber" que aquele jogador ficou livre demais. É
exatamente a classe de defeito que nosso
[torneio automatizado de táticas em CI](04-motor-de-partida.md#43-anti-exploit-a-lição-mais-cara-do-gênero)
existe para pegar **antes** do lançamento — a tolerância de 62% de aproveitamento máximo
para qualquer tática candidata é calibrada assumindo que padrões como o "Diablo"
*vão* aparecer nos testes, não que talvez apareçam.

---

## 2. Elifoot 98 (André Elias, Portugal, 1998)

### 2.1 O que o motor é

O Elifoot é o extremo oposto de complexidade: sem 2D, sem texto de comentário rico,
"apenas letras e números" — e mesmo assim se tornou o manager mais jogado da comunidade
lusófona por 25+ anos, o que por si só já é um dado de design relevante: **profundidade
de sistema não é sinônimo de riqueza visual**.

Uma série recente de artigos técnicos (autor: trsthales, publicados no TabNews) fez
engenharia reversa do binário original — compilado em Borland Delphi 1.0 (16-bit) — com
Ghidra e Cheat Engine, e documentou publicamente a lógica do motor de simulação. Os
pontos relevantes para nós:

* **Força por setor é distribuída por posição, sem mistura**: zagueiros entregam 100% da
  força ao setor de Defesa, atacantes 100% ao setor de Ataque, e apenas os
  meio-campistas dividem sua força 50/50 entre Ataque e Defesa.
* **Resultado da partida ≈ 70% de fatores acumulados** (força dos atletas + moral da
  equipe + mando de campo + bônus de nacionalidade + esquema tático) **vs. ≈ 30% de
  aleatoriedade pura** — uma proporção determinística/estocástica publicamente
  documentada que é um dado empírico útil de calibração (nosso próprio alvo de
  "correlação CA×pontos" em [`04 §4.1`](04-motor-de-partida.md#41-alvos-futebol-europeu-de-primeira-divisão-médias-recentes)
  mira exatamente esse tipo de equilíbrio, por um caminho de calibração diferente).
* **Vantagem de mando via divisor assimétrico**: o time da casa usa um divisor menor no
  sorteio de chances, o que os autores da engenharia reversa calculam resultar em
  aproximadamente **+16,7% de chances de criar lances de gol** em relação ao visitante —
  mando de campo como **viés estrutural no sorteio**, não como bônus aditivo simples.
* **Bônus de nacionalidade**: jogador com a mesma nacionalidade da liga do clube recebe
  **+3 pontos fixos** de bônus no seu setor — um mecanismo simples e legível de dar
  identidade a ligas nacionais.
* **Comportamento do jogador** (sujo/fair play) afeta faltas, cartões **e valor de
  mercado** simultaneamente — atributo oculto com efeito cruzado entre partida e
  economia, o mesmo princípio dos nossos
  [atributos ocultos](03-modelo-de-dados.md#32-atributos-ocultos-n_hidden--10).

### 2.2 O exploit "5-0-5" — o caso mais instrutivo deste documento inteiro

Como não existe, no motor do Elifoot, nenhuma penalidade por ausência de meio-campo
(nenhum mecanismo de posse/construção de jogo amarrado à presença de meio-campistas), a
formação **5 zagueiros – 0 meio-campistas – 5 atacantes** maximiza *simultaneamente* a
força bruta de Defesa e a força bruta de Ataque, sem nenhum custo estrutural. É descrito
pela própria comunidade do jogo como uma tática "quebra-jogo" — e é a demonstração mais
limpa possível de um princípio de design:

> **Um motor que soma força por setor precisa de um mecanismo que amarre os setores
> entre si (posse, construção, transição) — senão o "ótimo" do sistema deixa de ser uma
> escalação de futebol e vira um problema de otimização em duas variáveis.**

Isso valida diretamente uma decisão que já estava em [`04 §2.2`](04-motor-de-partida.md#22-camada-de-posse--cadeia-de-markov-espacial):
nosso motor **não soma forças de setor independentes** — ele resolve **posse por zona em
cadeia de Markov**, onde progredir da defesa ao ataque depende de vencer, zona a zona, a
marcação do adversário. Um "5-0-5" no nosso modelo não teria meio-campo para **vencer a
zona intermediária** e perderia posse sistematicamente antes de finalizar — a ausência
estrutural aparece como custo, não como economia.

### 2.3 Tabela-resumo do que herdamos do Elifoot

| Lição | Onde entra no nosso design |
|---|---|
| Interface densa em texto/números pode carregar 25 anos de comunidade sem gráfico nenhum | Valida o modo texto como cidadão de primeira classe, não só um fallback (RF-PA-05) |
| Força por setor sem mecanismo de ligação entre setores é explorável de forma trivial | Motivo estrutural da escolha de Markov por zona em vez de soma de setores ([`04 §2.2`](04-motor-de-partida.md#22-camada-de-posse--cadeia-de-markov-espacial)) |
| Mando de campo como viés no sorteio (não bônus aditivo) é simples de implementar e de entender | Inspira o modificador de mando em [`04 §2.1`](04-motor-de-partida.md#21-camada-tática--calculada-uma-vez-por-partida-e-a-cada-mudança) |
| Bônus de nacionalidade simples dá sabor a ligas locais sem complexidade extra | Candidato a modificador tático em `packs/core/engine.toml` (dado, não código — RNF-23) |
| Comportamento oculto com efeito cruzado partida↔mercado é barato e eficaz | Já coberto pelos [atributos ocultos](03-modelo-de-dados.md#32-atributos-ocultos-n_hidden--10) |
| Toda tática "dominante" documentada publicamente vira caso de teste permanente | Alimenta a [suíte anti-exploit](08-qualidade-e-testes.md#5-testes-anti-exploit) desde o primeiro commit do motor |

---

## 3. Síntese — o que os dois casos têm em comum

Championship Manager 03/04 é um sistema **rico e granular** (36+ atributos, CA/PA,
instruções individuais, 2D) que ainda assim vazou um exploit dominante porque uma
combinação específica de instruções encontrou um ponto cego da IA de marcação.
Elifoot 98 é um sistema **deliberadamente simples** (soma de força por setor) que vazou
um exploit ainda mais óbvio pela ausência completa de um mecanismo de ligação entre
setores.

A conclusão de design não é "seja mais simples" nem "seja mais complexo" — é:

> **Todo motor que resolve força por comparação de vetores precisa de um teste
> automatizado que jogue as combinações degeneradas antes que um jogador humano as
> encontre.** Nenhum dos dois times de 2003/2004 tinha esse teste em CI. Nós temos, desde
> [`04 §4.3`](04-motor-de-partida.md#43-anti-exploit-a-lição-mais-cara-do-gênero), como
> requisito de lançamento — não como resposta a um bug relatado pela comunidade dois anos
> depois.

Os casos "Diablo" e "5-0-5" **entram como casos de teste nomeados** na suíte anti-exploit
descrita em [`08-qualidade-e-testes.md §5`](08-qualidade-e-testes.md#5-testes-anti-exploit):
mesmo sem reproduzir o motor original, se o *nosso* motor produzir uma tática análoga —
um jogador estruturalmente desmarcado por uma combinação de instruções, ou uma formação
sem meio-campo que ainda assim domina — o torneio automatizado precisa acusar antes de
qualquer pessoa jogar.

---

## Referências consultadas

* [Championship Manager: Season 03/04 — Wikipedia](https://en.wikipedia.org/wiki/Championship_Manager:_Season_03/04) — motor 2D herdado do CM4, recepção da crítica
* [Championship Manager: Season 03/04 Review — GameSpot](https://www.gamespot.com/reviews/championship-manager-season-03-04-review/1900-6090044/) — descrição do motor 2D e sua eficácia
* [Champ Man 03/04, the 'Diablo' tactic & when Frank Lampard made it real — planetfootball.com](https://www.planetfootball.com/nostalgia/how-frank-lampard-turned-a-champ-manager-cheat-into-real-life) — histórico do exploit "Diablo"
* [Championship Manager: Season 03/04 — Guide (Chris_Pol) — GameFAQs](https://gamefaqs.gamespot.com/pc/918869-championship-manager-season-03-04/faqs/81791) — mecânicas de atributos e treino
* [CA15 Attributes vs Ability CA/PA — Championship Manager 2001/2002 Forums](https://champman0102.net/viewtopic.php?t=4251) — discussão de comunidade sobre CA/PA na linhagem CM
* [Potential Ability — Championship Manager Wiki (Fandom)](https://championshipmanager.fandom.com/wiki/Potential_Ability) — definição de PA como valor fixo atribuído no editor
* [Elifoot — Wikipédia](https://pt.wikipedia.org/wiki/Elifoot) — história e autoria (André Elias, Portugal, 1987–1998)
* [Elifoot 98 e a febre dos Simuladores de Futebol — Jogo Véio](https://jogoveio.com.br/elifoot-98-pc/) — contexto histórico e de comunidade
* [Engenharia Reversa: Como o motor do Elifoot 98 decide os gols (e por que o 5-0-5 quebra o jogo) — trsthales, TabNews](https://www.tabnews.com.br/trsthales/engenharia-reversa-como-o-motor-do-elifoot-98-decide-os-gols-e-por-que-o-5-0-5-quebra-o-jogo) — engenharia reversa do binário original (Ghidra), fórmulas de força por setor, bônus de nacionalidade, assimetria de mando, exploit 5-0-5
* [Desvendando o Elifoot 98 (Parte 2) — trsthales, TabNews](https://www.tabnews.com.br/trsthales/desvendando-o-elifoot-98-parte-2-como-montar-um-laboratorio-de-engenharia-reversa-de-16-bits-no-debian-12-com-ghidra-winevdm-e-cheat-engine) — metodologia da engenharia reversa
* [Guia Elifoot 98: Comportamento dos Jogadores — elifoot98.blogspot.com](http://elifoot98.blogspot.com/2010/12/comportamento-do-jogador.html) — efeito do comportamento sujo/fair play em faltas, cartões e mercado
