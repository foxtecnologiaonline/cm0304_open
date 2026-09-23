# Contribuindo

Obrigado pelo interesse. Este projeto está em **fase de escopo**: ainda não há código de
produção, e a contribuição mais valiosa agora é **revisar os documentos** em
[`docs/`](docs/) e abrir issues com discordâncias fundamentadas.

## Regra inegociável — clean-room

> Não use, não consulte e não redistribua **nenhum** arquivo, código-fonte, base de dados,
> asset ou documentação interna do *Championship Manager 03/04* ou de qualquer outro
> produto comercial de terceiros.

Isso inclui: bases comunitárias derivadas do jogo original, valores de atributos extraídos
dele, arquivos `.dat`/`.edt`/`.ddt`, arte, sons e textos. Se você teve acesso a
código-fonte ou documentação interna desses produtos, **não contribua para o núcleo de
simulação** — a contaminação por derivação é irreversível e afetaria todo mundo.

Todo PR terá uma confirmação explícita desta regra. Ver [`docs/05-dados-e-legal.md`](docs/05-dados-e-legal.md).

## Procedência das contribuições

Usamos **DCO**, não CLA. Assine seus commits:

```bash
git commit -s -m "feat(engine): ..."
```

## Antes de abrir um PR (quando houver código)

- [ ] `cargo fmt && cargo clippy -D warnings && cargo test`
- [ ] `flutter analyze && flutter test`, se tocou a interface
- [ ] Golden masters verificam, **ou** o PR explica a mudança de balanceamento e faz bump
      de `sim_version` ([`docs/08`](docs/08-qualidade-e-testes.md#3-golden-masters))
- [ ] Nenhum `f32`/`f64` em caminho que afete estado ([ADR 0002](docs/adr/0002-determinismo.md))
- [ ] Nenhuma regra de jogo codificada onde poderia ser dado (RNF-23)
- [ ] Documentação atualizada no mesmo PR (RNF-26)
- [ ] Nenhum texto fixo em código fora do arquivo de localização

## Decisões arquiteturais

Mudanças de stack, formato de save, contrato de FFI ou qualquer coisa que afete
determinismo exigem uma **ADR** em [`docs/adr/`](docs/adr/) antes da implementação.

## Convenções

* Commits: [Conventional Commits](https://www.conventionalcommits.org/) (`feat:`, `fix:`,
  `docs:`, `perf:`, `refactor:`, `test:`, `chore:`), escopo entre parênteses.
* Branches: `feat/<assunto>`, `fix/<assunto>`, `docs/<assunto>`.
* Trunk-based: branches curtas, PRs pequenos, revisão obrigatória.
* Documentos e comentários em **português**; identificadores de código em **inglês**.
