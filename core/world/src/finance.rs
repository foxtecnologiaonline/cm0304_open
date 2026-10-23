//! Orçamento de clube para o mercado (`crate::market`) — hoje sempre
//! sintético: o pack ainda não declara `finance`/orçamento (`docs/03 §1`
//! lista `CLUB ||--|| FINANCE` no modelo de domínio, mas nenhum campo
//! disso existe em `pack::ResolvedClub` ainda). Mesma filosofia de
//! `crate::strength`'s força sintética: um substituto sorteado
//! deterministicamente, documentado, não uma simulação de finanças de
//! verdade. Folha salarial ([`pay_salaries`]) é a primeira fatia real de
//! despesa recorrente — fatia mínima de RF-CL-02 (`docs/01 §2.2`): o
//! salário usa o CA travado no contrato de cada jogador
//! (`crate::contract::ContractState::signed_ability`), não o CA corrente —
//! ver `crate::contract` para luvas/cláusula ainda não existirem.
//! Bilheteria ([`match_day_revenue`]) é a primeira fatia real de receita —
//! fatia mínima de RF-CL-03 (`docs/01 §2.2`): só o ingresso de partida em
//! casa. Premiação ([`distribute_prize_money`]) é a segunda: um valor
//! fixo por competição por temporada, dividido entre os clubes conforme
//! a posição final na tabela (1º lugar ganha mais, último ganha menos).
//! TV ([`distribute_tv_revenue`]) é a terceira: outro valor fixo por
//! competição por temporada, mas dividido por **reputação**, não
//! posição — clube reputado atrai audiência o ano inteiro, mesmo fora
//! das primeiras colocações. Sem patrocínio ainda. Dívida
//! ([`repay_debt`]) é a fatia mínima do lado de "despesa/saldo" de
//! RF-CL-03: o que [`pay_salaries`] não consegue debitar do caixa vira
//! passivo em vez de evaporar, pago automaticamente na temporada
//! seguinte antes de qualquer outro gasto — sem juros, sem prazo, sem
//! renegociação.

use domain::{ClubId, DeterministicRng, Money};
use pack::LoadedPack;
use rules::TableRow;

use crate::contract::ContractState;
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
/// RF-CL-02 (`docs/01 §2.2`): sem luvas, sem cláusula de
/// rescisão/gols/aparições. Quem chama decide **qual** CA usar — ver
/// [`pay_salaries`], que usa o CA travado no contrato (`crate::contract`),
/// não o corrente.
#[must_use]
pub fn salary_per_season(ability: u8) -> Money {
    Money::from_cents(i64::from(ability) * CENTS_PER_CA_PER_SEASON)
}

/// Debita a folha salarial da temporada: todo jogador do `roster` —
/// titular ou reserva, saudável, lesionado ou suspenso, o clube paga o
/// elenco inteiro — desconta `salary_per_season(CA travado no contrato)`
/// do orçamento do seu clube. `contracts` tem que estar no mesmo índice
/// denso que `roster` (`crate::contract`, invariante do módulo); `debt`
/// no índice denso de `budgets`/`ClubId`. Devolve o total de fato pago do
/// caixa (pode ser menor que a soma nominal dos salários — um orçamento
/// sem caixa suficiente nunca fica negativo, `Money::saturating_sub`): a
/// diferença não evapora, vira dívida (`debt[club]`, fatia mínima de
/// RF-CL-03) — ver [`repay_debt`] pra como ela é paga de volta.
///
/// Diferente de uma transferência (`crate::market`), isto **não conserva
/// dinheiro** entre clubes: a folha é um gasto real que sai da economia
/// modelada (jogadores não têm orçamento próprio aqui), não uma troca
/// entre duas partes — o total de `budgets` só pode diminuir ou ficar
/// igual a cada chamada, nunca aumentar (a dívida criada é passivo, não
/// dinheiro que ainda existe em algum lugar).
pub fn pay_salaries(
    roster: &[PlayerState],
    contracts: &[ContractState],
    budgets: &mut [Money],
    debt: &mut [Money],
) -> Money {
    let mut total_paid = Money::ZERO;
    for (player, contract) in roster.iter().zip(contracts) {
        let salary = salary_per_season(contract.signed_ability);
        let idx = player.club.as_usize();
        let paid = budgets[idx].min(salary);
        let shortfall = salary - paid; // `Money::sub` já satura em zero
        budgets[idx] = budgets[idx] - salary;
        debt[idx] = debt[idx] + shortfall;
        total_paid = total_paid + paid;
    }
    total_paid
}

/// Paga a dívida acumulada de cada clube ([`pay_salaries`]) com o caixa
/// disponível — chamada antes de qualquer outro gasto da temporada
/// (`app::GameSession::dispatch`, antes de `pay_salaries` desta mesma
/// temporada): dívida vem primeiro, igual contabilidade de verdade.
/// Fatia mínima de "dívida" de RF-CL-03 (`docs/01 §2.2`): sem juros, sem
/// prazo, sem renegociação — só um desconto automático até o limite do
/// que o clube tem no caixa agora (nunca deixa o caixa negativo, mesma
/// disciplina de [`pay_salaries`]). `budgets` e `debt` têm que estar no
/// mesmo índice denso (`ClubId`). Devolve o total de fato pago.
pub fn repay_debt(budgets: &mut [Money], debt: &mut [Money]) -> Money {
    let mut total_repaid = Money::ZERO;
    for (budget, debt) in budgets.iter_mut().zip(debt.iter_mut()) {
        let repayment = (*budget).min(*debt);
        *budget = *budget - repayment;
        *debt = *debt - repayment;
        total_repaid = total_repaid + repayment;
    }
    total_repaid
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

/// Piso e teto de público por partida, em milésimos da capacidade do
/// estádio — nunca casa cheia nem vazia. O piso de verdade usado por
/// [`match_day_revenue`] é mais alto que este pra clube de reputação
/// maior (ver [`attendance_floor_permille`]) — este é só o piso absoluto,
/// de um clube sem reputação nenhuma.
const MIN_ATTENDANCE_PERMILLE: u32 = 200;
const MAX_ATTENDANCE_PERMILLE: u32 = 950;

/// Quanto a reputação pode empurrar o piso de público pra cima, em
/// milésimos — fatia de "receita" de RF-MU-08 (`docs/01 §2.4`: "Reputação
/// de país/clube influenciando atratividade, **receita** e IA"): clube
/// mais reputado atrai público mais consistente (nunca joga pra
/// arquibancada vazia), clube sem reputação some entre o piso absoluto e
/// a casa cheia. Não calibrado, só a direção certa — "atratividade" e
/// "IA" de RF-MU-08 ainda faltam (afetariam mercado/moral, não só
/// bilheteria).
const REPUTATION_ATTENDANCE_BOOST_PERMILLE: u32 = 500;

/// Piso de público efetivo de um clube, em milésimos da capacidade —
/// `MIN_ATTENDANCE_PERMILLE` mais uma fração de
/// `REPUTATION_ATTENDANCE_BOOST_PERMILLE` proporcional à reputação
/// (`crate::reputation::generate_club_reputation`, 0-100). Sempre menor
/// que `MAX_ATTENDANCE_PERMILLE` (200 + 500 = 700 no teto de reputação),
/// pra sempre sobrar faixa de sorteio.
fn attendance_floor_permille(reputation: u8) -> u32 {
    MIN_ATTENDANCE_PERMILLE + u32::from(reputation) * REPUTATION_ATTENDANCE_BOOST_PERMILLE / 100
}

/// Receita de bilheteria de uma partida de liga em casa — fatia mínima de
/// RF-CL-03 (`docs/01 §2.2`): só o clube mandante recebe (sem "cota de
/// visitante"), só bilheteria (sem TV, prêmio ou patrocínio). O público
/// sorteado é influenciado pela reputação do mandante
/// (`reputation`, 0-100 — `crate::reputation::generate_club_reputation`,
/// RF-MU-08): quanto maior, mais alto o piso de público (nunca mais alto
/// que `MAX_ATTENDANCE_PERMILLE`, nunca garante casa cheia). Determinístico
/// por `(world_seed, clube mandante, temporada, rodada)` —
/// `crate::discipline::season_round_tick` reaproveitado em vez de duplicado
/// (ver o doc de lá); `"world.attendance"` é um domínio de RNG próprio, então
/// nunca colide com `"world.discipline"` mesmo usando o mesmo tick
/// (`docs/02 §5`).
#[must_use]
pub fn match_day_revenue(
    home_club: ClubId,
    stadium_capacities: &[u32],
    club_reputation: &[u8],
    world_seed: u64,
    season_index: u32,
    round: u32,
) -> Money {
    let capacity = stadium_capacities[home_club.as_usize()];
    let reputation = club_reputation[home_club.as_usize()];
    let mut rng = DeterministicRng::seeded(
        world_seed,
        "world.attendance",
        u64::from(home_club.index()),
        crate::discipline::season_round_tick(season_index, round),
    );
    let floor = attendance_floor_permille(reputation);
    let span = MAX_ATTENDANCE_PERMILLE - floor;
    let attendance_permille = floor + rng.below(span);
    let attendance = capacity * attendance_permille / 1000;
    Money::from_cents(i64::from(attendance) * CENTS_PER_TICKET)
}

/// Pool total de premiação de uma competição de liga por temporada, em
/// centavos — fatia de "prêmio" de RF-CL-03 (`docs/01 §2.2`): magnitude
/// modesta por propósito (pequena fração da bilheteria/folha salarial de
/// uma competição no pack de exemplo), não dado calibrado.
const PRIZE_POOL_CENTS: i64 = 2_000_000; // 20.000,00

/// Premiação de uma posição final `position` (1 = campeão) numa tabela de
/// `n_clubs`, em centavos — peso linear decrescente (`n_clubs - position +
/// 1`): o campeão pesa `n_clubs` vezes mais que o lanterna. Arredonda pra
/// baixo (`PRIZE_POOL_CENTS` nem sempre se divide exatamente pelo peso
/// total — a sobra fica retida, não é dívida de ninguém).
fn prize_for_position(position: u32, n_clubs: u32) -> i64 {
    let weight = i64::from(n_clubs - position + 1);
    let total_weight = i64::from(n_clubs) * i64::from(n_clubs + 1) / 2;
    PRIZE_POOL_CENTS * weight / total_weight
}

/// Distribui a premiação de fim de temporada de uma competição de liga —
/// fatia de "prêmio" de RF-CL-03: `table[0]` (campeão) recebe a maior
/// fração do pool fixo ([`PRIZE_POOL_CENTS`]), o lanterna a menor,
/// decrescendo linearmente por posição ([`prize_for_position`]). `table`
/// tem que já estar na ordem de posição final (`rules::compute_table`,
/// como toda tabela deste crate) — o índice denso **é** a posição
/// (`idx + 1`). Devolve o total de fato distribuído (pode ser menor que
/// `PRIZE_POOL_CENTS` por arredondamento pra baixo). Tabela vazia
/// (competição sem clube — não deveria acontecer com um pack validado)
/// devolve `Money::ZERO` sem tocar `budgets`. Só liga: copa
/// (`crate::cup`) não tem tabela, não entra aqui.
#[must_use]
pub fn distribute_prize_money(table: &[TableRow], budgets: &mut [Money]) -> Money {
    let n_clubs = table.len() as u32;
    if n_clubs == 0 {
        return Money::ZERO;
    }
    let mut total_paid = Money::ZERO;
    for (idx, row) in table.iter().enumerate() {
        let position = idx as u32 + 1;
        let prize = Money::from_cents(prize_for_position(position, n_clubs));
        let club_idx = row.club.as_usize();
        budgets[club_idx] = budgets[club_idx] + prize;
        total_paid = total_paid + prize;
    }
    total_paid
}

/// Pool total de receita de TV de uma competição de liga por temporada,
/// em centavos — fatia de "TV" de RF-CL-03 (`docs/01 §2.2`): magnitude
/// modesta por propósito (mesma ordem de grandeza da premiação), não
/// dado calibrado.
const TV_REVENUE_POOL_CENTS: i64 = 3_000_000; // 30.000,00

/// Distribui a receita de TV de fim de temporada de uma competição de
/// liga — fatia de "TV" de RF-CL-03: ao contrário da premiação (por
/// posição final) e da bilheteria (por público de cada partida em casa),
/// TV reflete apelo de audiência o ano inteiro — dividida entre os
/// clubes de `clubs` **proporcionalmente à reputação** de cada um
/// (`club_reputation`, 0-100, `crate::reputation::generate_club_reputation`),
/// não à posição na tabela: um clube reputado que caiu pra última
/// posição ainda atrai mais audiência que um sem nome que subiu.
/// Peso `reputação + 1` (nunca zero, até o clube de reputação mínima
/// recebe uma fração) — arredonda pra baixo, a sobra fica retida.
/// `clubs` vazio devolve `Money::ZERO` sem tocar `budgets`. Só liga:
/// copa (`crate::cup`) não entra aqui, mesma exclusão de
/// [`distribute_prize_money`].
#[must_use]
pub fn distribute_tv_revenue(
    clubs: &[ClubId],
    club_reputation: &[u8],
    budgets: &mut [Money],
) -> Money {
    if clubs.is_empty() {
        return Money::ZERO;
    }
    let weight_of = |club: ClubId| u32::from(club_reputation[club.as_usize()]) + 1;
    let total_weight: u32 = clubs.iter().map(|&c| weight_of(c)).sum();
    let mut total_paid = Money::ZERO;
    for &club in clubs {
        let share = TV_REVENUE_POOL_CENTS * i64::from(weight_of(club)) / i64::from(total_weight);
        let payment = Money::from_cents(share);
        let idx = club.as_usize();
        budgets[idx] = budgets[idx] + payment;
        total_paid = total_paid + payment;
    }
    total_paid
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

    /// Contrato travado no mesmo CA de `player(..., ca)` — pros testes de
    /// `pay_salaries` abaixo que não testam a própria trava de contrato,
    /// só a folha, o CA travado bate com o CA do jogador.
    fn contract(ca: u8) -> ContractState {
        ContractState {
            seasons_remaining: 3,
            signed_ability: ca,
        }
    }

    #[test]
    fn pay_salaries_debita_o_orcamento_do_clube_certo() {
        let roster = vec![player(ClubId::new(0), 100), player(ClubId::new(1), 50)];
        let contracts = vec![contract(100), contract(50)];
        let mut budgets = vec![Money::from_cents(10_000_000), Money::from_cents(10_000_000)];
        let mut debt = vec![Money::ZERO; 2];

        let total_paid = pay_salaries(&roster, &contracts, &mut budgets, &mut debt);

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
        // Caixa sobrou pros dois — nenhuma dívida nova.
        assert_eq!(debt, vec![Money::ZERO; 2]);
    }

    #[test]
    fn pay_salaries_usa_o_ca_travado_no_contrato_nao_o_corrente() {
        // O jogador "cresceu" pra CA 150, mas o contrato ainda está
        // travado no CA 100 de quando foi assinado — a folha tem que
        // seguir o contrato, não o CA corrente.
        let roster = vec![player(ClubId::new(0), 150)];
        let contracts = vec![contract(100)];
        let mut budgets = vec![Money::from_cents(10_000_000)];
        let mut debt = vec![Money::ZERO; 1];
        let total_paid = pay_salaries(&roster, &contracts, &mut budgets, &mut debt);
        assert_eq!(total_paid, salary_per_season(100));
        assert_ne!(total_paid, salary_per_season(150));
    }

    #[test]
    fn pay_salaries_paga_o_elenco_inteiro_nao_so_titulares() {
        // Dois reservas do mesmo clube, nenhum titulariza sozinho (só um
        // jogador por posição não enche a formação) — ambos precisam ser
        // cobrados mesmo assim.
        let roster = vec![player(ClubId::new(0), 100), player(ClubId::new(0), 80)];
        let contracts = vec![contract(100), contract(80)];
        let mut budgets = vec![Money::from_cents(10_000_000)];
        let mut debt = vec![Money::ZERO; 1];
        let _ = pay_salaries(&roster, &contracts, &mut budgets, &mut debt);
        assert_eq!(
            budgets[0].cents(),
            10_000_000 - salary_per_season(100).cents() - salary_per_season(80).cents()
        );
    }

    #[test]
    fn pay_salaries_sem_caixa_suficiente_vira_dividida_em_vez_de_evaporar() {
        let roster = vec![player(ClubId::new(0), 200)]; // salário bem maior que o caixa
        let contracts = vec![contract(200)];
        let mut budgets = vec![Money::from_cents(100)];
        let mut debt = vec![Money::ZERO; 1];
        let total_paid = pay_salaries(&roster, &contracts, &mut budgets, &mut debt);
        assert_eq!(budgets[0], Money::ZERO);
        // Só o que de fato havia no caixa foi "pago", não o salário
        // nominal inteiro.
        assert_eq!(total_paid, Money::from_cents(100));
        // A diferença entre o salário nominal e o que foi pago vira
        // dívida — não evapora.
        assert_eq!(debt[0], salary_per_season(200) - Money::from_cents(100));
    }

    #[test]
    fn pay_salaries_nunca_aumenta_o_total_de_dinheiro() {
        let roster = vec![player(ClubId::new(0), 120), player(ClubId::new(1), 90)];
        let contracts = vec![contract(120), contract(90)];
        let mut budgets = vec![Money::from_cents(5_000_000), Money::from_cents(5_000_000)];
        let mut debt = vec![Money::ZERO; 2];
        let total_before: i64 = budgets.iter().map(|b| b.cents()).sum();

        let _ = pay_salaries(&roster, &contracts, &mut budgets, &mut debt);

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

    // Reputação "de meio de tabela" — testes que não são sobre o efeito
    // da reputação em si usam este valor pra não precisar pensar nisso.
    const MID_REPUTATION: u8 = 50;

    #[test]
    fn match_day_revenue_e_sempre_positiva_e_dentro_do_teto_da_capacidade() {
        let capacities = vec![MAX_SYNTHETIC_CAPACITY];
        let reputation = vec![MID_REPUTATION];
        for round in 0..50u32 {
            let revenue = match_day_revenue(ClubId::new(0), &capacities, &reputation, 1, 0, round);
            assert!(revenue.cents() > 0);
            // Nunca mais do que casa cheia pagando o preço do ingresso.
            assert!(revenue.cents() <= i64::from(MAX_SYNTHETIC_CAPACITY) * CENTS_PER_TICKET);
        }
    }

    #[test]
    fn match_day_revenue_e_deterministica_para_a_mesma_entrada() {
        let capacities = vec![20_000];
        let reputation = vec![MID_REPUTATION];
        assert_eq!(
            match_day_revenue(ClubId::new(0), &capacities, &reputation, 42, 3, 5),
            match_day_revenue(ClubId::new(0), &capacities, &reputation, 42, 3, 5)
        );
    }

    #[test]
    fn match_day_revenue_varia_com_a_rodada() {
        let capacities = vec![20_000];
        let reputation = vec![MID_REPUTATION];
        let revenues: Vec<Money> = (0..50)
            .map(|round| match_day_revenue(ClubId::new(0), &capacities, &reputation, 99, 0, round))
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
        let reputation = vec![MID_REPUTATION];
        let trials = 200u32;
        let small_total: i64 = (0..trials)
            .map(|round| {
                match_day_revenue(ClubId::new(0), &small, &reputation, 1, 0, round).cents()
            })
            .sum();
        let big_total: i64 = (0..trials)
            .map(|round| match_day_revenue(ClubId::new(0), &big, &reputation, 1, 0, round).cents())
            .sum();
        assert!(big_total > small_total);
    }

    #[test]
    fn match_day_revenue_cresce_com_a_reputacao_em_media() {
        // Mesma sanidade grosseira de `..._cresce_com_a_capacidade_em_media`,
        // agora pra reputação: clube mais reputado deveria, em média, gerar
        // mais bilheteria (piso de público mais alto), nunca menos.
        let capacities = vec![MAX_SYNTHETIC_CAPACITY];
        let low_reputation = vec![0u8];
        let high_reputation = vec![100u8];
        let trials = 200u32;
        let low_total: i64 = (0..trials)
            .map(|round| {
                match_day_revenue(ClubId::new(0), &capacities, &low_reputation, 1, 0, round).cents()
            })
            .sum();
        let high_total: i64 = (0..trials)
            .map(|round| {
                match_day_revenue(ClubId::new(0), &capacities, &high_reputation, 1, 0, round)
                    .cents()
            })
            .sum();
        assert!(high_total > low_total);
    }

    /// Linha de tabela mínima pra `distribute_prize_money` — só `club`
    /// importa (posição vem do índice denso em `table`, não de nenhum
    /// outro campo da linha).
    fn row(club: u32) -> TableRow {
        TableRow {
            club: ClubId::new(club),
            played: 0,
            wins: 0,
            draws: 0,
            losses: 0,
            goals_for: 0,
            goals_against: 0,
        }
    }

    #[test]
    fn distribute_prize_money_premia_mais_quem_termina_em_primeiro() {
        let table = vec![row(0), row(1), row(2)];
        let mut budgets = vec![Money::ZERO; 3];
        let _ = distribute_prize_money(&table, &mut budgets);
        assert!(budgets[0] > budgets[1]);
        assert!(budgets[1] > budgets[2]);
    }

    #[test]
    fn distribute_prize_money_credita_o_clube_certo_nao_a_posicao() {
        // Clube 2 termina em primeiro (índice 0 da tabela) — o orçamento
        // dele, não o do clube 0, tem que receber a maior premiação.
        let table = vec![row(2), row(0), row(1)];
        let mut budgets = vec![Money::ZERO; 3];
        let _ = distribute_prize_money(&table, &mut budgets);
        assert!(budgets[2] > budgets[0]);
        assert!(budgets[2] > budgets[1]);
    }

    #[test]
    fn distribute_prize_money_nunca_distribui_mais_que_o_pool() {
        let table: Vec<TableRow> = (0..8).map(row).collect();
        let mut budgets = vec![Money::ZERO; 8];
        let total_paid = distribute_prize_money(&table, &mut budgets);
        assert!(total_paid.cents() <= PRIZE_POOL_CENTS);
        assert!(total_paid.cents() > 0);
    }

    #[test]
    fn distribute_prize_money_total_pago_bate_com_a_soma_dos_orcamentos() {
        let table: Vec<TableRow> = (0..5).map(row).collect();
        let mut budgets = vec![Money::ZERO; 5];
        let total_paid = distribute_prize_money(&table, &mut budgets);
        let total_in_budgets: i64 = budgets.iter().map(|b| b.cents()).sum();
        assert_eq!(total_in_budgets, total_paid.cents());
    }

    #[test]
    fn distribute_prize_money_tabela_vazia_nao_distribui_nada() {
        let mut budgets: Vec<Money> = vec![];
        let total_paid = distribute_prize_money(&[], &mut budgets);
        assert_eq!(total_paid, Money::ZERO);
    }

    #[test]
    fn distribute_tv_revenue_premia_mais_quem_e_mais_reputado() {
        let clubs = vec![ClubId::new(0), ClubId::new(1), ClubId::new(2)];
        let club_reputation = vec![10u8, 50, 100];
        let mut budgets = vec![Money::ZERO; 3];
        let _ = distribute_tv_revenue(&clubs, &club_reputation, &mut budgets);
        assert!(budgets[2] > budgets[1]);
        assert!(budgets[1] > budgets[0]);
    }

    #[test]
    fn distribute_tv_revenue_ignora_posicao_na_tabela() {
        // Diferente de `distribute_prize_money`: a ordem de `clubs` (que
        // pra premiação seria a posição final) não deveria importar aqui
        // — só a reputação de cada um.
        let clubs_a = vec![ClubId::new(0), ClubId::new(1)];
        let clubs_b = vec![ClubId::new(1), ClubId::new(0)];
        let club_reputation = vec![20u8, 80];

        let mut budgets_a = vec![Money::ZERO; 2];
        let _ = distribute_tv_revenue(&clubs_a, &club_reputation, &mut budgets_a);

        let mut budgets_b = vec![Money::ZERO; 2];
        let _ = distribute_tv_revenue(&clubs_b, &club_reputation, &mut budgets_b);

        assert_eq!(budgets_a, budgets_b);
    }

    #[test]
    fn distribute_tv_revenue_reputacao_minima_ainda_recebe_algo() {
        let clubs = vec![ClubId::new(0), ClubId::new(1)];
        let club_reputation = vec![0u8, 100];
        let mut budgets = vec![Money::ZERO; 2];
        let _ = distribute_tv_revenue(&clubs, &club_reputation, &mut budgets);
        assert!(
            budgets[0].cents() > 0,
            "clube de reputação mínima ainda deveria receber uma fração (peso reputação + 1)"
        );
    }

    #[test]
    fn distribute_tv_revenue_nunca_distribui_mais_que_o_pool() {
        let clubs: Vec<ClubId> = (0..8).map(ClubId::new).collect();
        let club_reputation = vec![50u8; 8];
        let mut budgets = vec![Money::ZERO; 8];
        let total_paid = distribute_tv_revenue(&clubs, &club_reputation, &mut budgets);
        assert!(total_paid.cents() <= TV_REVENUE_POOL_CENTS);
        assert!(total_paid.cents() > 0);
    }

    #[test]
    fn distribute_tv_revenue_total_pago_bate_com_a_soma_dos_orcamentos() {
        let clubs: Vec<ClubId> = (0..5).map(ClubId::new).collect();
        let club_reputation = vec![30u8, 40, 50, 60, 70];
        let mut budgets = vec![Money::ZERO; 5];
        let total_paid = distribute_tv_revenue(&clubs, &club_reputation, &mut budgets);
        let total_in_budgets: i64 = budgets.iter().map(|b| b.cents()).sum();
        assert_eq!(total_in_budgets, total_paid.cents());
    }

    #[test]
    fn distribute_tv_revenue_sem_clubes_nao_distribui_nada() {
        let mut budgets: Vec<Money> = vec![];
        let total_paid = distribute_tv_revenue(&[], &[], &mut budgets);
        assert_eq!(total_paid, Money::ZERO);
    }

    #[test]
    fn repay_debt_paga_o_que_o_caixa_permite_e_reduz_a_divida() {
        let mut budgets = vec![Money::from_cents(300)];
        let mut debt = vec![Money::from_cents(1_000)];
        let total_repaid = repay_debt(&mut budgets, &mut debt);
        assert_eq!(total_repaid, Money::from_cents(300));
        assert_eq!(budgets[0], Money::ZERO);
        assert_eq!(debt[0], Money::from_cents(700));
    }

    #[test]
    fn repay_debt_zera_a_divida_quando_o_caixa_cobre_tudo() {
        let mut budgets = vec![Money::from_cents(5_000)];
        let mut debt = vec![Money::from_cents(1_000)];
        let total_repaid = repay_debt(&mut budgets, &mut debt);
        assert_eq!(total_repaid, Money::from_cents(1_000));
        assert_eq!(budgets[0], Money::from_cents(4_000));
        assert_eq!(debt[0], Money::ZERO);
    }

    #[test]
    fn repay_debt_sem_divida_nenhuma_nao_muda_o_caixa() {
        let mut budgets = vec![Money::from_cents(5_000)];
        let mut debt = vec![Money::ZERO];
        let total_repaid = repay_debt(&mut budgets, &mut debt);
        assert_eq!(total_repaid, Money::ZERO);
        assert_eq!(budgets[0], Money::from_cents(5_000));
    }

    #[test]
    fn repay_debt_nunca_deixa_caixa_nem_divida_negativos() {
        let mut budgets = vec![Money::ZERO];
        let mut debt = vec![Money::from_cents(1_000)];
        let total_repaid = repay_debt(&mut budgets, &mut debt);
        assert_eq!(total_repaid, Money::ZERO);
        assert_eq!(budgets[0], Money::ZERO);
        assert_eq!(debt[0], Money::from_cents(1_000));
    }

    #[test]
    fn pagar_divida_depois_salario_fecha_o_ciclo_de_uma_temporada_em_debito() {
        // Simula o que `app::GameSession::dispatch` faz em duas
        // temporadas seguidas: temporada 1 sem caixa (vira dívida);
        // temporada 2 com caixa de sobra, mas `repay_debt` roda antes de
        // `pay_salaries` (dívida vem primeiro), então o salário da
        // temporada 2 só usa o que sobrou depois de pagar a dívida da
        // temporada 1.
        let roster = vec![player(ClubId::new(0), 200)];
        let contracts = vec![contract(200)];
        let mut budgets = vec![Money::from_cents(100)];
        let mut debt = vec![Money::ZERO; 1];

        let _ = pay_salaries(&roster, &contracts, &mut budgets, &mut debt);
        let outstanding = salary_per_season(200) - Money::from_cents(100);
        assert_eq!(debt[0], outstanding);

        // Temporada 2: caixa de sobra chega (ex.: bilheteria), mas a
        // dívida é paga primeiro.
        budgets[0] = budgets[0] + outstanding + Money::from_cents(1);
        let repaid = repay_debt(&mut budgets, &mut debt);
        assert_eq!(repaid, outstanding);
        assert_eq!(debt[0], Money::ZERO);
        assert_eq!(budgets[0], Money::from_cents(1));
    }
}
