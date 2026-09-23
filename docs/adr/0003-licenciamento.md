# ADR 0003 — MPL-2.0 no código, CC0 nos dados

**Status:** Proposta · **Data:** 2026-09-23 · **Decidir antes do primeiro commit de código**

## Contexto

O projeto é aberto e quer distribuir em **App Store e Google Play** (RNF-20). Copyleft
forte (GPLv2/GPLv3) entra em conflito prático com os termos de uso e o DRM da App Store —
conflito que já levou à remoção de aplicativos GPL da loja da Apple. Adotar GPL criaria
um bloqueio autoinfligido na plataforma mais restritiva.

Ao mesmo tempo, o projeto quer que melhorias no **núcleo de simulação** — que é o ativo
real — retornem à comunidade, e não sejam absorvidas por um fork fechado.

Relicenciar depois exige consentimento de todos os contribuidores. A decisão é agora.

## Decisão

| Componente | Licença |
|---|---|
| Núcleo, interface, ferramentas | **MPL-2.0** |
| SDK de modding, exemplos, especificação do formato de pack | **Apache-2.0** |
| Data packs oficiais | **CC0-1.0** |
| Assets gerados (SVG, ícones) | **CC-BY-SA-4.0** |

Contribuições sob **DCO** (`Signed-off-by`), sem CLA. `cargo-deny` em CI bloqueia
dependências com licença incompatível (GPL/AGPL/proprietária) entrando por transitividade.

## Alternativas consideradas

* **GPLv3** — melhor copyleft, incompatível na prática com a App Store. Recusada por R2 de alvo.
* **Apache-2.0 puro** — máxima adoção, mas permite fork fechado do núcleo sem retorno.
* **MIT** — mesmo problema, sem concessão de patentes.
* **GPL + exceção de loja** — funciona, mas a exceção precisa ser redigida com apoio
  jurídico e assusta contribuidores. Complexidade desproporcional ao ganho.

## Consequências

**Positivas** — distribuição viável nas três plataformas; melhorias por arquivo no núcleo
voltam ao projeto; SDK permissivo estimula ferramentas de terceiros; dados circulam sem
atrito.

**Negativas** — MPL é copyleft mais fraco: um produto comercial pode embrulhar o núcleo
em um app fechado, desde que publique as modificações dos arquivos MPL. Consideramos
aceitável — o valor do projeto está na comunidade e nos dados, não em impedir uso.
