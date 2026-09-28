#!/usr/bin/env python3
"""Gerador determinístico de `people/*.json` para o pack de exemplo (`ex`).

Este NÃO é o pipeline de dados de verdade do projeto (esse ainda não existe
— ver `docs/05-dados-e-legal.md §3.3`). É um gerador placeholder, só para
este pack de desenvolvimento/CI: nomes e atributos inventados, sem nenhuma
relação com jogadores reais, usados apenas para exercitar
`pack::resolve_players` e `world::strength_from_squad` de ponta a ponta
(`docs/07-roadmap.md`, M1/M2).

Uso: `python3 generate_people.py` a partir deste diretório — regrava todos
os arquivos em `../people/`. Determinístico: a mesma versão deste script
sempre produz os mesmos arquivos (seed fixa por clube), então rodar de novo
sem editar o script não deveria gerar diff nenhum.
"""

from __future__ import annotations

import json
import random
from pathlib import Path

CLUBS = [
    "es.t1.01",
    "es.t1.02",
    "es.t1.03",
    "es.t1.04",
    "es.t1.05",
    "es.t1.06",
    "es.t1.07",
    "es.t1.08",
    "es.t2.01",
    "es.t2.02",
    "es.t2.03",
    "es.t2.04",
    "es.t2.05",
    "es.t2.06",
    "es.t2.07",
    "es.t2.08",
]
NATION = "es"

FIRST_NAMES = [
    "Adão", "Bento", "Caio", "Danilo", "Edu", "Fábio", "Gil", "Hugo",
    "Iago", "Juca", "Kaio", "Léo", "Mateus", "Nando", "Otávio", "Pedro",
    "Quim", "Rui", "Sérgio", "Tiago", "Ubiratã", "Valter", "Wander", "Xisto",
    "Yago", "Zeca", "Aurélio", "Bruno", "Cássio", "Davi", "Emanuel", "Firmino",
]
LAST_NAMES = [
    "Aguiar", "Barros", "Cavalcante", "Dornelles", "Esteves", "Farias",
    "Guimarães", "Homem", "Ibiapina", "Junqueira", "Kessler", "Leal",
    "Maciel", "Nogueira", "Osório", "Pimentel", "Queiroga", "Ramalho",
    "Siqueira", "Teixeira", "Uchoa", "Vasconcelos", "Wagner", "Xavier",
    "Yepes", "Zambrano", "Abranches", "Brandão", "Cordeiro", "Dutra",
]

# (posição, quantidade, faixa de idade, atributos-chave, faixa de valor dos
# atributos-chave, faixa de valor dos demais atributos declarados)
SQUAD_PLAN = [
    ("gk", 2, (18, 34), ["reflexes", "rushing", "aerial_ability", "distribution", "positioning", "concentration"]),
    ("df", 5, (17, 33), ["tackling", "marking", "heading", "strength", "positioning", "anticipation", "pace", "stamina"]),
    ("mf", 5, (17, 32), ["passing", "vision", "first_touch", "dribbling", "stamina", "decisions", "teamwork", "technique"]),
    ("fw", 4, (17, 31), ["finishing", "dribbling", "pace", "off_the_ball", "composure", "acceleration", "technique"]),
]


def make_player(rng: random.Random, club: str, idx: int, position: str, age_range: tuple[int, int], key_attrs: list[str]) -> dict:
    first = rng.choice(FIRST_NAMES)
    last = rng.choice(LAST_NAMES)
    age = rng.randint(*age_range)
    birth_year = 2026 - age
    birth = f"{birth_year:04d}-{rng.randint(1, 12):02d}-{rng.randint(1, 28):02d}"

    attrs = {name: rng.randint(11, 18) for name in key_attrs}
    # Um punhado de atributos genéricos fora do grupo-chave, pra o elenco
    # não parecer artificialmente unidimensional.
    for name in rng.sample(["determination", "leadership", "agility", "balance", "composure", "concentration"], 3):
        attrs.setdefault(name, rng.randint(8, 15))

    current = rng.randint(70, 155)
    potential = min(200, current + rng.randint(0, 45))

    return {
        "id": f"{club}.p{idx:02d}",
        "club": club,
        "nation": NATION,
        "first_name": first,
        "last_name": last,
        "birth_date": birth,
        "position": position,
        "attributes": attrs,
        "ability": {"current": current, "potential": potential},
    }


def generate_club(club: str) -> list[dict]:
    rng = random.Random(f"managerfc.example.{club}")
    players = []
    idx = 1
    for position, count, age_range, key_attrs in SQUAD_PLAN:
        for _ in range(count):
            players.append(make_player(rng, club, idx, position, age_range, key_attrs))
            idx += 1
    return players


def main() -> None:
    out_dir = Path(__file__).resolve().parent.parent / "people"
    out_dir.mkdir(exist_ok=True)
    for club in CLUBS:
        players = generate_club(club)
        path = out_dir / f"{club}.json"
        path.write_text(json.dumps(players, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        print(f"{path}: {len(players)} jogadores")


if __name__ == "__main__":
    main()
