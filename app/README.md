# `app/` — interface Flutter do ManagerFC

Cliente Flutter descrito em [`docs/02-arquitetura.md §1`](../docs/02-arquitetura.md#1-decisão-de-stack)
e [`docs/06-ui-ux.md`](../docs/06-ui-ux.md). Roda em Windows 10/11, Android e iOS a
partir do mesmo código.

## Estado atual

Este diretório contém, por enquanto, **só o esqueleto de fonte** (`pubspec.yaml`,
`lib/main.dart`, `analysis_options.yaml`) — o suficiente para ler e revisar, mas **não
verificado por build**: este ambiente de desenvolvimento não tem o SDK do Flutter
instalado, então nada aqui foi rodado com `flutter analyze`, `flutter test` ou
`flutter build`.

**Faltam as pastas de plataforma nativa** (`android/`, `ios/`, `windows/`, e os
metadados de `linux/`/`macos/` que vêm de brinde). Elas são geradas pela própria
ferramenta, não escritas à mão — gerar isso manualmente sem poder compilar seria pior
que não ter nada, porque pareceria pronto sem estar.

## Como continuar (com Flutter instalado)

```bash
cd app
flutter create --project-name managerfc --org dev.managerfc \
  --platforms windows,android,ios .   # gera android/, ios/, windows/ preservando lib/ e pubspec.yaml
flutter pub get
flutter analyze
flutter test
flutter run -d windows   # ou -d <device-id> para Android/iOS
```

Depois disso, o próximo passo do M0 é a ponte com o núcleo:

1. Adicionar `flutter_rust_bridge_codegen` e gerar o binding a partir de `core/app`
   (`docs/02 §1.1` e `§4` — contrato `dispatch`/`query`/`events`).
2. Ligar `main.dart` ao binding gerado, substituindo a tela placeholder atual.
3. Cobrir com `flutter test` os widgets de dado descritos em
   [`docs/06 §6`](../docs/06-ui-ux.md#6-design-system).

## Regra que não muda

Nenhuma regra de jogo mora aqui — se um cálculo decide algo (quem vence uma disputa,
quanto custa um jogador, se uma tática é válida), a resposta vem de uma `Query` ao
núcleo em `core/`, nunca de lógica escrita em Dart (`docs/02 §4`).
