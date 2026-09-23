# 05 — Dados, licenciamento e jurídico

> **Aviso:** este documento é análise de engenharia, não parecer jurídico. As
> recomendações abaixo reduzem risco; antes de publicar em loja, o projeto deve passar
> por revisão de um advogado de propriedade intelectual na jurisdição de distribuição.

Este é o capítulo que mais frequentemente mata projetos deste tipo — normalmente **depois**
de anos de desenvolvimento. Por isso ele é decidido antes da primeira linha de código.

---

## 1. Situação da marca e do produto original

| Fato | Consequência para nós |
|---|---|
| Em 2003/2004 a Sports Interactive se separou da Eidos: a SI ficou com o **código-fonte, a base de dados e o motor**, e a Eidos ficou com a **marca *Championship Manager***. | O produto de 2003 está inteiramente coberto por direitos de terceiros — código, base e marca, em mãos diferentes. |
| A Eidos foi comprada pela Square Enix e, em **2 de maio de 2022**, franquias da Eidos (incluindo *Championship Manager*) foram vendidas ao **Embracer Group**. | O titular da marca hoje é o Embracer Group. Usar "Championship Manager", "CM 03/04" ou derivados no nome, ícone, loja ou material de divulgação é risco direto. |
| A SI seguiu com *Football Manager*, usando a base e o motor retidos. | Não há qualquer versão "abandonada" ou liberada do produto: nada está em domínio público. |

### 1.1 Regras invioláveis do projeto

| # | Regra | Motivo |
|---|---|---|
| L1 | **Nenhum arquivo do jogo original** (`.exe`, `.dat`, `.edt`, `.ddt`, saves, arte, som) entra no repositório, nem em ferramentas, nem em testes. | Cópia/derivação de obra protegida. |
| L2 | **Nenhum importador ou conversor** desses arquivos é distribuído pelo projeto. | Facilitar a extração cria responsabilidade própria. |
| L3 | **Nenhum colaborador** com acesso a código-fonte, documentação interna ou assets do original contribui para o núcleo (regra de *clean-room*). | Evita contaminação por derivação. |
| L4 | **A marca não é usada** no nome do produto, do app, do domínio ou do material de divulgação. O codinome do repositório (`cm0304_open`) **não** vai para a loja. | Direito marcário. |
| L5 | Comparações ("inspirado nos managers clássicos dos anos 2000") são permitidas; alegação de continuidade, compatibilidade ou endosso, não. | Uso nominativo x confusão de origem. |
| L6 | Nenhum número, tabela ou fórmula é copiado de análise/engenharia reversa da base original. | Ver §2. |

Reimplementar **mecânicas** é legítimo: ideias, regras e sistemas de jogo não são
protegidos por direito autoral — a expressão é. Nome, arte, texto, código e base de dados
são expressão. É por isso que o projeto pode recriar *a sensação* e não pode recriar
*o conteúdo*.

---

## 2. Nome do produto

O repositório continua `cm0304_open` (codinome interno). O produto precisa de nome próprio.

| Candidato | Comentário |
|---|---|
| **Touchline** | Curto, tema claro, sem colisão óbvia com o gênero — recomendado |
| **Boot Room** | Referência ao Liverpool clássico, tom nostálgico |
| **Gaffer** | Coloquial britânico para "o treinador"; verificar uso prévio em apps |
| **Prancheta** | Alternativa em PT-BR, boa para o mercado local |

Antes de fixar: busca em INPI (BR), EUIPO/UKIPO, USPTO, Google Play e App Store, e
verificação de domínio. **Tarefa do M0**, não do lançamento.

---

## 3. Fontes de dados

### 3.1 O que usamos

| Camada | Fonte | Licença | Observação |
|---|---|---|---|
| Ligas, temporadas, resultados históricos | **openfootball / football.json** | domínio público (sem exigência de atribuição) | Estruturas e calendários; sem chave de API |
| Clubes, estádios, pessoas (dados factuais) | **Wikidata** | CC0 | Nome, data de nascimento, nacionalidade, clube |
| Estatísticas de eventos para calibração | **StatsBomb Open Data** e similares | licença própria, **uso não comercial/atribuição — verificar por dataset** | Usado para *calibrar o motor*, não embarcado no jogo |
| **Atributos 1–20, CA/PA** | **geração própria** + curadoria da comunidade | CC0 (obra do projeto) | Ver §3.3 |
| Escudos, cores, uniformes | **gerados proceduralmente** (SVG) | CC0 | Nunca assets oficiais |
| Nomes para regens | dicionários de nomes por país, montados de fontes abertas | CC0 | |

### 3.2 O que **não** usamos

* Raspagem de sites cujos termos de uso proíbem (a maioria dos portais comerciais de
  estatística) — inclusive "só para gerar a base inicial".
* Bases de dados de outros jogos (do original ou de produtos atuais), mesmo as
  publicadas pela comunidade.
* Fotos de jogadores, escudos oficiais, uniformes, logos de competição, hinos.
* Bases sob licença que exija compartilhamento incompatível com a distribuição em lojas.

### 3.3 Como nascem os atributos (o problema mais interessante)

Sem base licenciada, os ~36 atributos de 250 mil jogadores precisam vir de algum lugar.
Pipeline proposto (`tools/ratings/`):

```
estatísticas públicas por jogador (minutos, gols, assistências, desarmes, competição)
   → normalização por posição, minutagem e força da liga
   → modelo que projeta CA a partir de desempenho + idade + nível da competição
   → distribuição de CA em atributos, guiada pelo perfil da posição e pelo estilo inferido
   → curadoria da comunidade (votação/revisão com histórico e reversão)
   → publicação como data pack CC0, versionado
```

Propriedades desejáveis: é **nosso** (sem derivação de terceiros), é auditável (cada
valor tem proveniência), melhora com o tempo e transforma a comunidade de consumidora em
coautora — que é o modelo que mantém vivo esse gênero há vinte anos.

Onde o dado público não alcança (divisões inferiores, futebol de base), a base é
**gerada proceduralmente** com distribuições plausíveis por país e divisão, marcada
explicitamente como fictícia.

---

## 4. Nomes reais de jogadores e clubes

O ponto de maior risco residual, e onde a decisão precisa ser consciente.

| Aspecto | Situação |
|---|---|
| **Fatos** (nome, nascimento, nacionalidade, clube, estatísticas) | Fatos não são protegidos por direito autoral. Uma **coleção** pode ser protegida (direito *sui generis* de banco de dados na UE/UK), mas construir a nossa a partir de fontes abertas evita isso. |
| **Direito de personalidade / NIL** | Nome, imagem e semelhança de pessoas reais são protegidos em muitas jurisdições. A indústria licencia via **FIFPro** e associações nacionais — algo fora do alcance de um projeto sem receita. Há litígio real na área (ex.: disputas sobre uso de NIL em jogos de futebol). |
| **Marcas de clubes** | Nomes e escudos de clubes são marcas registradas. Usar o escudo é claramente arriscado; usar o nome é uso descritivo, mas não é risco zero. |

**Estratégia adotada — três modos, escolha do usuário:**

1. **Modo genérico (padrão em lojas oficiais).** Base completa com jogadores e clubes
   **fictícios**, gerados proceduralmente, com estrutura de ligas real (formatos de
   competição não são protegidos). Zero risco de NIL e de marca. É o que o app publica.
2. **Modo comunidade (pack aberto, distribuído fora da loja).** Data pack CC0 com nomes
   factuais, **sem** escudos, fotos ou uniformes. Instalado por escolha explícita do
   usuário. Publicado no repositório de packs, não dentro do app.
3. **Packs de terceiros.** O jogo carrega packs locais do usuário. O projeto não hospeda,
   não indexa e não endossa packs de conteúdo que não controle.

Essa separação é a mesma lógica que preserva projetos abertos de longa vida: **o motor é
neutro, o conteúdo é do usuário**. Também limita o dano de um eventual *takedown*: ele
atingiria um pack, nunca o jogo.

---

## 5. Licenciamento do código

### 5.1 O problema da GPL e da App Store

Copyleft forte (GPLv2/GPLv3) entra em conflito prático com os termos das lojas da Apple,
que impõem restrições de uso e DRM incompatíveis com as liberdades exigidas pela
licença — conflito que já resultou na remoção de aplicativos GPL da App Store. Como
**iOS é plataforma-alvo (RNF-20)**, adotar GPL criaria um bloqueio autoinfligido.

### 5.2 Recomendação

| Componente | Licença | Por quê |
|---|---|---|
| Núcleo, UI, ferramentas | **MPL-2.0** | Copyleft por arquivo: melhorias no núcleo voltam para o projeto, mas a distribuição em lojas permanece viável |
| SDK de modding, exemplos, formato de pack | **Apache-2.0** | Máxima adoção; inclui concessão explícita de patentes |
| Data packs oficiais | **CC0-1.0** (dados) | Dado deve circular sem atrito |
| Assets gerados (SVG, ícones, fontes próprias) | **CC-BY-SA-4.0** | Preserva a identidade visual |

Registro: [ADR 0003](adr/0003-licenciamento.md). A decisão precisa ser tomada **antes**
do primeiro commit de código — relicenciar depois exige consentimento de todos os
contribuidores.

### 5.3 Contribuições

* **DCO** (`Signed-off-by`) em vez de CLA — menos atrito, suficiente para procedência.
* `CONTRIBUTING.md` com a regra de *clean-room* (L3) explícita e caixa de confirmação no
  template de PR.
* Dependências auditadas em CI (`cargo-deny`): bloqueia licenças incompatíveis (GPL,
  AGPL, proprietárias) entrando por transitividade.

---

## 6. Privacidade e lojas

| Item | Decisão |
|---|---|
| Coleta de dados | Nenhuma por padrão. Telemetria **opt-in**, anônima, com o que é enviado documentado e visível na UI (RNF-17). |
| Contas / login | Inexistentes. |
| Anúncios / compras | Inexistentes (RF-W-04). |
| Classificação etária | Livre/3+ esperado; sem violência, sem apostas, sem chat. Declarar corretamente evita rejeição. |
| Ficha de privacidade (Apple/Google) | "Nenhum dado coletado" — declaração simples e verdadeira, desde que a telemetria permaneça opt-in. |
| Conteúdo gerado pelo usuário | Packs são locais; o app não hospeda nem distribui conteúdo de terceiros — evita as regras de moderação de UGC das lojas. |

---

## 7. Plano de resposta a notificação

Improvável no modo genérico, mas definido antecipadamente:

1. **Ponto de contato** público e responsivo no repositório.
2. Ao receber notificação: **não apagar histórico**; avaliar com apoio jurídico;
   responder no prazo.
3. Se procedente e restrita a conteúdo: remover o **pack**, manter o jogo. É exatamente
   para isso que a separação motor/conteúdo (§4) existe.
4. Comunicar a comunidade com transparência e registro público da decisão.

---

## 8. Checklist antes de qualquer publicação

- [ ] Nome do produto verificado em INPI/EUIPO/USPTO e nas duas lojas
- [ ] Licenças de código e de dados definidas e aplicadas em todos os arquivos
- [ ] `cargo-deny` verde; nenhuma dependência com licença incompatível
- [ ] Build da loja em **modo genérico**, sem nomes reais embarcados
- [ ] Nenhum asset de terceiros no repositório (auditoria de arquivos binários)
- [ ] `CONTRIBUTING.md` com regra de clean-room e DCO ativo
- [ ] Ficha de privacidade preenchida e coerente com o código
- [ ] Revisão jurídica externa concluída e registrada

---

## Referências consultadas

* [Championship Manager — Wikipedia](https://en.wikipedia.org/wiki/Championship_Manager) (separação SI/Eidos, 2004)
* [Sports Interactive — Wikipedia](https://en.wikipedia.org/wiki/Sports_Interactive) (divisão de código e base de dados)
* [Championship Manager: Season 03/04 — Wikipedia](https://en.wikipedia.org/wiki/Championship_Manager:_Season_03/04) (escopo do produto original)
* [openfootball/football.json](https://github.com/openfootball/football.json) — dados de futebol em domínio público
* [openfootball/awesome-football](https://github.com/openfootball/awesome-football) — catálogo de datasets abertos
* [FIFA: How Does the Most Successful Sports Video Game Obtain Player Image Rights? — Fordham IPLJ](http://www.fordhamiplj.org/2021/11/11/fifa-how-does-the-most-successful-sports-video-game-obtain-player-i-rights/)
* [Foul play? Ibrahimović and the role of digital likeness in sports video games — Freshfields](https://technologyquotient.freshfields.com/post/102gpgj/foul-play-ibrahimovic-and-the-role-of-digital-likeness-in-sports-video-games)
