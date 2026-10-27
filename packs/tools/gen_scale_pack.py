#!/usr/bin/env python3
"""Gera um pack fictício em escala de MVP (docs/00 §7: 2 divisões, 48 clubes,
~1.440 jogadores) num diretório de saída — só para medir desempenho
(critério de MVP #2, RNF-02/08). A saída NÃO é versionada nem entra no app:
nomes e atributos inventados, mesma filosofia do pack de exemplo.

Uso: python3 gen_scale_pack.py <diretório-de-saída> [clubes-por-divisão=24]
Determinístico: mesma versão do script, mesma saída.
"""
from __future__ import annotations

import json
import random
import sys
from pathlib import Path

FIRST = ["Adão", "Bento", "Caio", "Danilo", "Edu", "Fábio", "Gil", "Hugo", "Iago", "Juca",
         "Kaio", "Léo", "Mateus", "Nando", "Otávio", "Pedro", "Quim", "Rui", "Sérgio", "Tiago"]
LAST = ["Aguiar", "Barros", "Cavalcante", "Dornelles", "Esteves", "Farias", "Guimarães",
        "Homem", "Junqueira", "Leal", "Maciel", "Nogueira", "Osório", "Pimentel", "Ramalho",
        "Siqueira", "Teixeira", "Vasconcelos", "Xavier", "Zambrano"]
PLAN = [
    ("gk", 3, (18, 34), ["reflexes", "rushing", "aerial_ability", "distribution", "positioning", "concentration"]),
    ("df", 10, (17, 33), ["tackling", "marking", "heading", "strength", "positioning", "anticipation", "pace", "stamina"]),
    ("mf", 10, (17, 32), ["passing", "vision", "first_touch", "dribbling", "stamina", "decisions", "teamwork", "technique"]),
    ("fw", 7, (17, 31), ["finishing", "dribbling", "pace", "off_the_ball", "composure", "acceleration", "technique"]),
]


def write(path: Path, obj) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(obj, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")


def player(rng: random.Random, club: str, idx: int, pos: str, ages: tuple[int, int], keys: list[str]) -> dict:
    age = rng.randint(*ages)
    attrs = {k: rng.randint(11, 18) for k in keys}
    for k in rng.sample(["determination", "leadership", "agility", "balance", "composure", "concentration"], 3):
        attrs.setdefault(k, rng.randint(8, 15))
    cur = rng.randint(70, 155)
    return {
        "id": f"{club}.p{idx:02d}", "club": club, "nation": "sc",
        "first_name": rng.choice(FIRST), "last_name": rng.choice(LAST),
        "birth_date": f"{2026 - age:04d}-{rng.randint(1, 12):02d}-{rng.randint(1, 28):02d}",
        "position": pos, "attributes": attrs,
        "ability": {"current": cur, "potential": min(200, cur + rng.randint(0, 45))},
    }


def main() -> None:
    out = Path(sys.argv[1])
    per = int(sys.argv[2]) if len(sys.argv) > 2 else 24
    (out / "pack.toml").parent.mkdir(parents=True, exist_ok=True)
    (out / "pack.toml").write_text(
        'id = "scale.two-tier"\nname = "Escala MVP"\nversion = "0.1.0"\n'
        'license = "CC0-1.0"\nreference_year = 2026\n', encoding="utf-8")
    write(out / "nations" / "sc.json", {"id": "sc", "name": "Escalônia"})
    # O validador exige copa com potência de 2 usando todos os clubes do
    # país (sem bye): 48 clubes ficam sem copa, 64 (--per 32) têm.
    total = 2 * per
    if total & (total - 1) == 0:
        write(out / "competitions" / "cup.json", {"id": "sc.cup", "name": "Copa", "nation": "sc", "tier": 0,
                                                   "format": {"type": "knockout", "teams": total}})
    tb = ["points", "wins", "goal_difference", "goals_for", "head_to_head"]
    write(out / "competitions" / "tier1.json", {
        "id": "sc.tier1", "name": "Divisão 1", "nation": "sc", "tier": 1,
        "format": {"type": "round_robin", "legs": 2, "teams": per}, "tiebreakers": tb,
        "relegation": {"to": "sc.tier2", "slots": 3}})
    write(out / "competitions" / "tier2.json", {
        "id": "sc.tier2", "name": "Divisão 2", "nation": "sc", "tier": 2,
        "format": {"type": "round_robin", "legs": 2, "teams": per}, "tiebreakers": tb,
        "promotion": {"to": "sc.tier1", "slots": 3}})
    for tier in (1, 2):
        for n in range(1, per + 1):
            club = f"sc.t{tier}.{n:02d}"
            write(out / "clubs" / f"{club}.json", {"id": club, "name": f"Clube {tier}-{n:02d}",
                                                    "nation": "sc", "competition": f"sc.tier{tier}", "founded": 1900 + n})
            rng = random.Random(f"managerfc.scale.{club}")
            squad, idx = [], 1
            for pos, count, ages, keys in PLAN:
                for _ in range(count):
                    squad.append(player(rng, club, idx, pos, ages, keys))
                    idx += 1
            write(out / "people" / f"{club}.json", squad)
    print(f"{out}: {2 * per} clubes, {2 * per * 30} jogadores")


if __name__ == "__main__":
    main()
