# Prioridade permanente: mobile leve e compacto

**O alvo principal do ManagerFC é mobile** (Android/iOS), não desktop. Toda
decisão de arquitetura, dependência ou formato de dado deve ser julgada
primeiro pelo custo que impõe num celular comum — não só "funciona", mas
"funciona leve". Isto vale para qualquer sessão futura neste repositório,
não só para quem pediu isto da última vez.

Na prática, isso significa:

* **Prefira a opção mais leve** entre alternativas equivalentes: menos
  dependências, menos alocação, menos I/O, payload menor — mesmo que a
  opção mais pesada seja mais simples de escrever. Ver
  [`docs/02-arquitetura.md`](docs/02-arquitetura.md) para o porquê de Rust +
  Flutter (sem GC no núcleo, binário único por plataforma) em vez de
  alternativas descartadas por pesarem mais no mobile (.NET MAUI, Tauri).
* **Respeite os orçamentos de performance e memória já definidos** —
  [`docs/01-requisitos.md §3`](docs/01-requisitos.md) (RNF-01 a RNF-08):
  note que o alvo mobile é sempre o mais apertado (ex.: RNF-08: ≤ 600 MB
  de RAM no mobile vs. ≤ 1,2 GB no desktop; RNF-02: ≤ 1,5 s no mobile vs.
  ≤ 400 ms no desktop). Uma fatia nova que bate o alvo desktop mas não o
  mobile **não está pronta**.
* **Evite inflar o tamanho do app/pack**: compressão de dados (zstd,
  `docs/02 §7`), carregar só o necessário em memória (`docs/02 §4`), sem
  assets pesados, sem dependências nativas grandes "de brinde".
* **Teste mentalmente contra um celular médio**, não contra a máquina de
  desenvolvimento — CPU mais lenta, RAM mais escassa, bateria/térmico
  limitando sustentação de carga.
* Ao propor a próxima fatia de qualquer sistema (motor, mundo, app, UI),
  mencione explicitamente se ela tem custo de CPU/memória/tamanho visível
  no mobile, e prefira a versão mínima que não compromete esse alvo.

Isto não substitui `docs/00-escopo.md`/`docs/02-arquitetura.md`/
`docs/01-requisitos.md` — é um lembrete permanente de que, entre essas
referências, a lente mobile-leve é a que sempre vence em caso de dúvida.
