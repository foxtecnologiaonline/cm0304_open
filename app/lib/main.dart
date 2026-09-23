// Ponto de entrada do ManagerFC (Flutter).
//
// Este arquivo é, por enquanto, um placeholder: prova que a árvore `app/`
// existe com a estrutura descrita em `docs/02-arquitetura.md §3`, mas ainda
// não fala com o núcleo em Rust — a ponte via `flutter_rust_bridge`
// (`docs/02 §1.1`) é o próximo passo do M0, documentado em `app/README.md`.
//
// Regra que vale desde a primeira tela: nenhuma lógica de jogo mora aqui.
// Se um cálculo decide algo, mora no núcleo (`core/`) — este arquivo só
// desenha.
import 'package:flutter/material.dart';

void main() {
  runApp(const ManagerFcApp());
}

/// Widget raiz do ManagerFC.
class ManagerFcApp extends StatelessWidget {
  const ManagerFcApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'ManagerFC',
      theme: ThemeData(colorSchemeSeed: Colors.green, useMaterial3: true),
      darkTheme: ThemeData(
        colorSchemeSeed: Colors.green,
        brightness: Brightness.dark,
        useMaterial3: true,
      ),
      home: const _ScaffoldingScreen(),
    );
  }
}

class _ScaffoldingScreen extends StatelessWidget {
  const _ScaffoldingScreen();

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('ManagerFC')),
      body: const Center(
        child: Padding(
          padding: EdgeInsets.all(24),
          child: Column(
            mainAxisAlignment: MainAxisAlignment.center,
            children: [
              Text(
                'M0 — Fundação',
                style: TextStyle(fontSize: 22, fontWeight: FontWeight.bold),
              ),
              SizedBox(height: 12),
              Text(
                'A interface ainda não fala com o núcleo. '
                'Veja docs/02-arquitetura.md e app/README.md.',
                textAlign: TextAlign.center,
              ),
            ],
          ),
        ),
      ),
    );
  }
}
