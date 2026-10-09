//! Orçamento de clube para o mercado (`crate::market`) — hoje sempre
//! sintético: o pack ainda não declara `finance`/orçamento (`docs/03 §1`
//! lista `CLUB ||--|| FINANCE` no modelo de domínio, mas nenhum campo
//! disso existe em `pack::ResolvedClub` ainda). Mesma filosofia de
//! `crate::strength`'s força sintética: um substituto sorteado
//! deterministicamente, documentado, não uma simulação de finanças de
//! verdade. Folha salarial ([`pay_salaries`]) é a primeira fatia real de
//! despesa recorrente — fatia mínima de RF-CL-02 (`docs/01 §2.2`): só o
//! custo, sem contrato, duração, luvas ou cláusula nenhuma. Bilheteria
//! ([`match_day_revenue`]) é a primeira fatia real de receita — fatia
//! mínima de RF-CL-03 (`docs/01 §2.2`): só o ingresso de partida em casa,
//! nada de TV, prêmio ou patrocínio ainda (o resto de RF-CL-03 fica para
//! quando essas fontes existirem).

use domain::{ClubId, DeterministicRng, Money};
use pack::LoadedPack;

use crate::progression::PlayerState;

/// Piso e teto do orçamento sintético, em centavos — magnitude escolhida
/// pra ficar na mesma ordem de grandeza de alguns jogadores de
/// `ai::market_value` (CA médio ~100 → ~1.000.000,00), não uma calibração
/// econômica de verdade.
const MIN_SYNTHETIC_BUDGET_CENTS: i64 = 10_000_000; // 100.000,00
const MAX_SYNTHETIC_BUDGET_CENTS: i64 = 1_000_000_000; // 10.000.000,00

/// Gera um orçamento por clube, indexado pelo id denso do clube
/// (`ClubId::as_usize()`) — mesmo layout de `crate::strength::generate_strengths`.
#[must_use]
pub fn generate_budgets(pack: &LoadedPack, world_seed: u64) -> Vec<Money> {
    pack.clubs
        .iter()
        .map(|club| synthetic_budget(club.id, world_seed))
        .collect()
}

fn synthetic_budget(club: ClubId, world_seed: u64) -> Money {
    let mut rng =
        DeterministicRng::seeded(world_seed, "world.club_budget", u64::from(club.index()), 0);
    let span = (MAX_SYNTHETIC_BUDGET_CENTS - MIN_SYNTHETIC_BUDGET_CENTS) as u32;
    let cents = MIN_SYNTHETIC_BUDGET_CENTS + i64::from(rng.below(span));
    Money::from_cents(cents)
}

/// Quanto cada ponto de CA custa de salário por temporada, em centavos —
/// linear, ao contrário de `ai::market_value` (quadrático): dobrar a
/// habilidade dobra o salário, não quadruplica. Escolhido só pra dar
/// números de magnitude plausível contra o orçamento sintético deste
/// módulo (um elenco médio drena uma fração minoritária do orçamento por
/// temporada, não tudo de uma vez) — não é dado calibrado, mesmo status
/// de `ai::market_value`.
const CENTS_PER_CA_PER_SEASON: i64 = 5_000;

/// Salário de um jogador por temporada, só função de CA — fatia mínima de
/// RF-CL-02 (`docs/01 §2.2`): sem contrato, sem duração, sem luvas, sem
/// cláusula de rescisão/gols/aparições. O resto de RF-CL-02 fica para
/// quando houver negociação de verdade (`RF-TR-03`/`04`, M2) para decidir
/// esses termos — aqui o "contrato" é implícito e eterno.
#[must_use]
pub fn salary_per_season(current_ability: u8) -> Money {
    Money::from_cents(i64::from(current_ability) * CENTS_PER_CA_PER_SEASON)
}

/// Debita a folha salarial da temporada: todo jogador do `roster` —
/// titular ou reserva, saudável, lesionado ou suspenso, o clube paga o
/// elenco inteiro — desconta `salary_per_season(CA atual)` do orçamento
/// do seu clube. Devolve o total de fato pago (pode ser menor que a soma
/// nominal dos salários: um orçamento sem caixa suficiente só zera,
/// nunca fica negativo — `Money::saturating_sub`, dívida de verdade é
/// RF-CL-03, não implementada).
///
/// Diferente de uma transferência (`crate::market`), isto **não conserva
/// dinheiro** entre clubes: a folha é um gasto real que sai da economia
/// modelada (jogadores não têm orçamento próprio aqui), não uma troca
/// entre duas partes — o total de `budgets` só pode diminuir ou ficar
/// igual a cada chamada, nunca aumentar.
pub fn pay_salaries(roster: &[PlayerState], budgets: &mut [Money]) -> Money {
    let mut total_paid = Money::ZERO;
    for player in roster {
        let salary = salary_per_season(player.ability.current());
        let idx = player.club.as_usize();
        let paid = budgets[idx].min(salary);
        budgets[idx] = budgets[idx] - salary;
        total_paid = total_paid + paid;
    }
    total_paid
}

/// Piso e teto de capacidade de estádio sintética, em pessoas — mesma
/// filosofia de `MIN_SYNTHETIC_BUDGET_CENTS`/`MAX_SYNTHETIC_BUDGET_CENTS`:
/// magnitude plausível (clubes pequenos a grandes), não dado real (o pack
/// só declara o *nome* do estádio — `pack::ResolvedClub::stadium` — nunca
/// capacidade; isso é RF-CL-05, M4, não implementado).
const MIN_SYNTHETIC_CAPACITY: u32 = 5_000;
const MAX_SYNTHETIC_CAPACITY: u32 = 60_000;

/// Gera a capacidade de estádio de cada clube, indexada pelo id denso do
/// clube — mesmo layout de [`generate_budgets`]. Chamada uma vez por
/// carreira (`app::GameSession::new`): não há obras de estádio ainda
/// (RF-CL-05, M4), então a capacidade de um clube nunca muda depois de
/// gerada.
#[must_use]
pub fn generate_stadium_capacities(pack: &LoadedPack, world_seed: u64) -> Vec<u32> {
    pack.clubs
        .iter()
        .map(|club| synthetic_capacity(club.id, world_seed))
        .collect()
}

fn synthetic_capacity(club: ClubId, world_seed: u64) -> u32 {
    let mut rng = DeterministicRng::seeded(
        world_seed,
        "world.stadium_capacity",
        u64::from(club.index()),
        0,
    );
    let span = MAX_SYNTHETIC_CAPACITY - MIN_SYNTHETIC_CAPACITY;
    MIN_SYNTHETIC_CAPACITY + rng.below(span)
}

/// Preço médio do ingresso, em centavos — **não** um preço realista (seria
/// R$0,30): é um parâmetro de balanceamento, mesmo status de
/// `CENTS_PER_CA_PER_SEASON`, escolhido pra manter o total de bilheteria de
/// uma temporada na mesma ordem de grandeza da folha salarial que ela
/// complementa (`pay_salaries`) — não dado calibrado nem realista. Com a
/// faixa de capacidade/público abaixo, a bilheteria fica deliberadamente
/// **menor** que a folha no pack de exemplo: a economia continua drenando
/// dinheiro ao longo de uma carreira, só mais devagar do que antes desta
/// fatia.
const CENTS_PER_TICKET: i64 = 30; // 0,30

/// Faixa de público por partida, em milésimos da capacidade do estádio —
/// nunca casa cheia nem vazia (sem reputação de clube, posição na tabela
/// ou rivalidade influenciando público ainda — `RF-MU-08`, futuro).
const MIN_ATTENDANCE_PERMILLE: u32 = 300;
const MAX_ATTENDANCE_PERMILLE: u32 = 950;

/// Receita de bilheteria de uma partida de liga em casa — fatia mínima de
/// RF-CL-03 (`docs/01 §2.2`): só o clube mandante recebe (sem "cota de
/// visitante"), só bilheteria (sem TV, prêmio ou patrocínio). Determinístico
/// por `(world_seed, clube mandante, temporada, rodada)` —
/// `crate::discipline::season_round_tick` reaproveitado em vez de duplicado
/// (ver o doc de lá); `"world.attendance"` é um domínio de RNG próprio, então
/// nunca colide com `"world.discipline"` mesmo usando o mesmo tick
/// (`docs/02 §5`).
#[must_use]
pub fn match_day_revenue(
    home_club: ClubId,
    stadium_capacities: &[u32],
    world_seed: u64,
    season_index: u32,
    round: u32,
) -> Money {
    let capacity = stadium_capacities[home_club.as_usize()];
    let mut rng = DeterministicRng::seeded(
        world_seed,
        "world.attendance",
        u64::from(home_club.index()),
        crate::discipline::season_round_tick(season_index, round),
    );
    let span = MAX_ATTENDANCE_PERMILLE - MIN_ATTENDANCE_PERMILLE;
    let attendance_permille = MIN_ATTENDANCE_PERMILLE + rng.below(span);
    let attendance = capacity * attendance_permille / 1000;
    Money::from_cents(i64::from(attendance) * CENTS_PER_TICKET)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn example_pack() -> LoadedPack {
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/example-two-tier");
        pack::load_and_validate(&path).unwrap().pack
    }

    #[test]
    fn gera_um_orcamento_por_clube_dentro_dos_limites() {
        let p = example_pack();
        let budgets = generate_budgets(&p, 42);
        assert_eq!(budgets.len(), p.clubs.len());
        for b in &budgets {
            assert!(b.cents() >= MIN_SYNTHETIC_BUDGET_CENTS);
            assert!(b.cents() <= MAX_SYNTHETIC_BUDGET_CENTS);
        }
    }

    #[test]
    fn e_deterministico_para_a_mesma_seed() {
        let p = example_pack();
        assert_eq!(generate_budgets(&p, 7), generate_budgets(&p, 7));
    }

    #[test]
    fn seeds_diferentes_dao_orcamentos_diferentes() {
        let p = example_pack();
        assert_ne!(generate_budgets(&p, 1), generate_budgets(&p, 2));
    }

    #[test]
    fn salario_cresce_linearmente_com_o_ca() {
        assert_eq!(
            salary_per_season(100).cents(),
            100 * CENTS_PER_CA_PER_SEASON
        );
        // Dobrar o CA dobra o salário — ao contrário de `ai::market_value`
        // (quadrático), que quadruplicaria.
        assert_eq!(
            salary_per_season(200).cents(),
            2 * salary_per_season(100).cents()
        );
    }

    fn player(club: ClubId, ca: u8) -> PlayerState {
        PlayerState {
            player: domain::PlayerId::new(0),
            club,
            position: domain::Position::Midfielder,
            age_years: 25,
            ability: domain::Ability::new(ca, 200),
            injured: false,
            condition: crate::condition::FULL_CONDITION,
        }
    }

    #[test]
    fn pay_salaries_debita_o_orcamento_do_clube_certo() {
        let roster = vec![player(ClubId::new(0), 100), player(ClubId::new(1), 50)];
        let mut budgets = vec![Money::from_cents(10_000_000), Money::from_cents(10_000_000)];

        let total_paid = pay_salaries(&roster, &mut budgets);

        assert_eq!(
            budgets[0].cents(),
            10_000_000 - salary_per_season(100).cents()
        );
        assert_eq!(
            budgets[1].cents(),
            10_000_000 - salary_per_season(50).cents()
        );
        assert_eq!(
            total_paid.cents(),
            salary_per_season(100).cents() + salary_per_season(50).cents()
        );
    }

    #[test]
    fn pay_salaries_paga_o_elenco_inteiro_nao_so_titulares() {
        // Dois reservas do mesmo clube, nenhum titulariza sozinho (só um
        // jogador por posição não enche a formação) — ambos precisam ser
        // cobrados mesmo assim.
        let roster = vec![player(ClubId::new(0), 100), player(ClubId::new(0), 80)];
        let mut budgets = vec![Money::from_cents(10_000_000)];
        pay_salaries(&roster, &mut budgets);
        assert_eq!(
            budgets[0].cents(),
            10_000_000 - salary_per_season(100).cents() - salary_per_season(80).cents()
        );
    }

    #[test]
    fn pay_salaries_nunca_deixa_orcamento_negativo() {
        let roster = vec![player(ClubId::new(0), 200)]; // salário bem maior que o caixa
        let mut budgets = vec![Money::from_cents(100)];
        let total_paid = pay_salaries(&roster, &mut budgets);
        assert_eq!(budgets[0], Money::ZERO);
        // Só o que de fato havia no caixa foi "pago", não o salário
        // nominal inteiro.
        assert_eq!(total_paid, Money::from_cents(100));
    }

    #[test]
    fn pay_salaries_nunca_aumenta_o_total_de_dinheiro() {
        let roster = vec![player(ClubId::new(0), 120), player(ClubId::new(1), 90)];
        let mut budgets = vec![Money::from_cents(5_000_000), Money::from_cents(5_000_000)];
        let total_before: i64 = budgets.iter().map(|b| b.cents()).sum();

        pay_salaries(&roster, &mut budgets);

        let total_after: i64 = budgets.iter().map(|b| b.cents()).sum();
        assert!(
            total_after < total_before,
            "folha salarial deveria drenar dinheiro da economia, nunca aumentá-lo"
        );
    }

    #[test]
    fn gera_uma_capacidade_por_clube_dentro_dos_limites() {
        let p = example_pack();
        let capacities = generate_stadium_capacities(&p, 42);
        assert_eq!(capacities.len(), p.clubs.len());
        for &c in &capacities {
            assert!(c >= MIN_SYNTHETIC_CAPACITY);
            assert!(c <= MAX_SYNTHETIC_CAPACITY);
        }
    }

    #[test]
    fn capacidade_e_deterministica_para_a_mesma_seed() {
        let p = example_pack();
        assert_eq!(
            generate_stadium_capacities(&p, 7),
            generate_stadium_capacities(&p, 7)
        );
    }

    #[test]
    fn match_day_revenue_e_sempre_positiva_e_dentro_do_teto_da_capacidade() {
        let capacities = vec![MAX_SYNTHETIC_CAPACITY];
        for round in 0..50u32 {
            let revenue = match_day_revenue(ClubId::new(0), &capacities, 1, 0, round);
            assert!(revenue.cents() > 0);
            // Nunca mais do que casa cheia pagando o preço do ingresso.
            assert!(revenue.cents() <= i64::from(MAX_SYNTHETIC_CAPACITY) * CENTS_PER_TICKET);
        }
    }

    #[test]
    fn match_day_revenue_e_deterministica_para_a_mesma_entrada() {
        let capacities = vec![20_000];
        assert_eq!(
            match_day_revenue(ClubId::new(0), &capacities, 42, 3, 5),
            match_day_revenue(ClubId::new(0), &capacities, 42, 3, 5)
        );
    }

    #[test]
    fn match_day_revenue_varia_com_a_rodada() {
        let capacities = vec![20_000];
        let revenues: Vec<Money> = (0..50)
            .map(|round| match_day_revenue(ClubId::new(0), &capacities, 99, 0, round))
            .collect();
        assert!(
            revenues.windows(2).any(|w| w[0] != w[1]),
            "50 rodadas produziram sempre a mesma receita — RNG de público não está variando"
        );
    }

    #[test]
    fn match_day_revenue_cresce_com_a_capacidade_em_media() {
        // Sanidade grosseira: um estádio maior deveria, em média, gerar
        // mais bilheteria que um menor, para muitas rodadas independentes —
        // não testa o valor exato (RNG varia o público dentro da faixa),
        // só a direção do efeito.
        let small = vec![MIN_SYNTHETIC_CAPACITY];
        let big = vec![MAX_SYNTHETIC_CAPACITY];
        let trials = 200u32;
        let small_total: i64 = (0..trials)
            .map(|round| match_day_revenue(ClubId::new(0), &small, 1, 0, round).cents())
            .sum();
        let big_total: i64 = (0..trials)
            .map(|round| match_day_revenue(ClubId::new(0), &big, 1, 0, round).cents())
            .sum();
        assert!(big_total > small_total);
    }
}
