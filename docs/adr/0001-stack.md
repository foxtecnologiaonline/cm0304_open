# ADR 0001 — Núcleo em Rust, interface em Flutter

**Status:** Proposta · **Data:** 2026-09-23 · **Decide:** equipe fundadora

## Contexto

O produto precisa rodar em Windows 10/11, Android e iOS com **um save só** e com
comportamento idêntico. O perfil de carga é incomum para um "jogo": pouquíssimo render,
muita simulação em lote (milhares de partidas por dia de calendário) e muita **UI de
dados** — tabelas de milhares de linhas, filtros, comparações.

As duas exigências puxam para lados opostos: engines de jogo resolvem o 2D e falham na UI
de dados; frameworks de app resolvem a UI e falham na simulação pesada e no determinismo.

## Decisão

Separar em duas tecnologias, com fronteira estreita:

* **Núcleo em Rust** — determinístico, sem GC, compila para x86-64, ARM64 e WASM; roda
  headless em CI. Contém 100% das regras.
* **Interface em Flutter** — um código para as três plataformas, com controle total de
  render (bom para listas densas e para o campo 2D em canvas).
* **Ponte via `flutter_rust_bridge`**, com superfície mínima: `dispatch`, `query`, `events`.

## Alternativas consideradas

| Alternativa | Motivo da recusa |
|---|---|
| Godot 4 | 2D excelente, UI de dados densos fraca; construiríamos um framework de tabelas dentro da engine |
| Unity | Licenciamento hostil a projeto aberto; runtime pesado para um jogo de listas |
| .NET MAUI / Avalonia | Forte no Windows; mobile (iOS sobretudo) é o elo fraco; pausas de GC atrapalham lote |
| Tauri 2 | WebView diferente por plataforma; DOM não alcança os alvos de listas grandes |
| Kotlin Multiplatform | Resolveria o núcleo, mas obrigaria três UIs nativas |
| C++ no núcleo | Equivalente em performance; Rust ganha em segurança de memória, ferramental e onboarding de contribuidores |

## Consequências

**Positivas** — núcleo testável sem UI; determinismo controlável; performance previsível
no mobile; troca de UI possível sem tocar em regras; WASM abre a porta para build web.

**Negativas** — duas linguagens e dois ecossistemas de build; `flutter_rust_bridge` é
dependência de risco; contribuidores precisam de Rust para mexer em regra de jogo.

**Mitigação** — contrato de FFI pequeno e estável, documentado; plano B (FFI manual)
esboçado; tarefas de entrada marcadas separadamente para núcleo e UI.
