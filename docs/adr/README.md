# Architecture Decision Records

Registro curto e imutável das decisões que são caras de reverter. Uma ADR nunca é
editada depois de aceita — é **substituída** por outra que a supersede.

| # | Decisão | Status |
|---|---|---|
| [0001](0001-stack.md) | Rust no núcleo + Flutter na interface | Proposta |
| [0002](0002-determinismo.md) | Determinismo como requisito, sem ponto flutuante no estado | Proposta |
| [0003](0003-licenciamento.md) | MPL-2.0 no código, CC0 nos dados | Proposta |
| [0004](0004-dados-proprios.md) | Base de dados própria, clean-room, modo genérico por padrão | Proposta |

Quando abrir uma ADR: mudança de stack, de formato de save, de modelo de
licenciamento, de contrato de FFI, ou qualquer coisa que afete determinismo (RNF-24).
