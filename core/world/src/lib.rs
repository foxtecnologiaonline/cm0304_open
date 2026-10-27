//! `world` — o loop que faz o mundo girar sozinho, sem UI: calendário,
//! temporada, tabela, promoção e rebaixamento, progressão de jogadores,
//! lesões e mercado. Consome um `pack::LoadedPack`, o `engine` de partida e
//! a `ai` de clube; é o portão do M1
//! (`docs/07-roadmap.md#m1--kick-off-headless-10-semanas`).
//!
//! Ainda não implementado: regens, aposentadoria, calendário mensal
//! (`docs/07-roadmap.md`). Clubes sem elenco no pack usam força e
//! orçamento sintéticos ([`strength`], [`finance`]), substitutos
//! documentados, não uma simulação de elenco. Copa ([`cup`]) é fase única,
//! sem sorteio de confronto e com empate decidido por moeda — escopo mais
//! estreito que uma copa de verdade, documentado em [`cup`]. Condição
//! ([`condition`]) é por temporada, não cumulativa e não cobre fadiga
//! acumulada de carreira nem frescor dia a dia — documentado em
//! [`condition`]. Moral ([`morale::apply_morale`], RF-JG-07 ao lado de
//! condição) é por clube, não por jogador: saldo de vitórias/derrotas nas
//! últimas 3 partidas de liga já disputadas na temporada escala a força um
//! pouco pra cima ou pra baixo — sem reação individual a minutos jogados
//! ou decisão do técnico (RF-JG-10, personalidade, M4). Suspensão
//! ([`discipline`]) foi a primeira coisa aqui com granularidade de rodada
//! (liga simulada rodada a rodada no caminho de carreira,
//! `season::run_season`) — sem cartão amarelo acumulado, sem reincidência,
//! documentado em [`discipline`]; jogo importante, consistência, adaptação
//! e moral reaproveitam o mesmo ponto de simulação depois. Folha salarial
//! ([`finance::pay_salaries`]) é a primeira despesa recorrente de verdade
//! — fatia mínima de RF-CL-02, sem contrato, duração, luvas ou cláusula.
//! Bilheteria ([`finance::match_day_revenue`]) é a primeira receita
//! recorrente de verdade — fatia mínima de RF-CL-03, por público de
//! cada partida em casa. Premiação ([`finance::distribute_prize_money`])
//! é a segunda: um pool fixo por competição de liga, dividido por
//! posição final (campeão ganha mais, lanterna ganha menos) uma vez por
//! temporada. TV ([`finance::distribute_tv_revenue`]) é a terceira: outro
//! pool fixo, dividido por **reputação** em vez de posição — clube
//! reputado atrai audiência o ano inteiro, mesmo fora das primeiras
//! colocações. Patrocínio ([`finance::pay_sponsorship`]) é a quarta: valor
//! absoluto por clube, escalado pela reputação, não um pool dividido
//! entre clubes. Dívida ([`finance::repay_debt`])
//! fecha o lado de despesa: salário que o caixa não cobre não evapora,
//! vira passivo, pago automaticamente na temporada seguinte antes de
//! qualquer outro gasto — sem juros, sem prazo. Duração de contrato
//! ([`contract`]) complementa a folha salarial: salário travado no CA do
//! momento da assinatura, não no CA corrente, só reajustando quando o
//! contrato de fato renova (automático, sem negociação) ou o jogador é
//! transferido — sem agente livre ainda (`RF-TR-07`, M3). Reputação de
//! clube ([`reputation`]) é a fatia mínima de RF-MU-08: só influencia
//! bilheteria hoje (público mais consistente pra clube mais reputado),
//! "atratividade" e "IA" de RF-MU-08 ainda faltam. Propensão a lesão
//! ([`injuries::generate_injury_proneness`]) é a primeira fatia real de
//! RF-JG-02 (atributos ocultos): chance de lesão varia por jogador, não
//! mais fixa em 6% pra todo mundo. Profissionalismo
//! ([`progression::generate_professionalism`]) é a segunda: bônus de
//! evolução por cima da curva de idade/minutos jogados. Lealdade
//! ([`market::generate_loyalty`]) é a terceira: jogador muito leal nunca
//! é candidato a saída em [`run_market_day`], mesmo que o preço bata.
//! Sujeira ([`discipline::generate_dirtiness`]) é a quarta: jogador mais
//! "sujo" tem mais chance de ser o escolhido quando o time sofre uma
//! expulsão em [`discipline::roll_suspensions`], em vez de sorteio
//! uniforme entre os titulares — a chance do time ser expulso continua a
//! mesma. Jogo importante ([`cup::generate_big_game_mentality`]) é a
//! quinta: time com titulares de mentalidade mais alta rende um pouco
//! mais na copa ([`cup::apply_big_game_bonus`], 0,9x a 1,1x de força),
//! sem nenhum efeito na liga. Consistência
//! ([`consistency::generate_consistency`]) é a sexta: time com titulares
//! inconsistentes balança até ±20% de força a cada rodada de liga
//! ([`consistency::apply_consistency_swing`], ruído zero-médio, não
//! direcionado); consistência máxima nunca balança. Sem efeito em copa
//! (sem granularidade de rodada lá, ver [`cup`]) nem no caminho estático.
//! Ambição ([`market::generate_ambition`]) é a sétima: jogador muito
//! ambicioso recusa ir pra um clube de reputação menor que a do seu
//! clube atual em [`run_market_day`], mesmo com preço e lealdade batendo;
//! pouco ambicioso aceita qualquer destino, igual a antes desta fatia.
//! Adaptação ([`adaptation::generate_adaptation`]) é a oitava e última:
//! recém-chegado de adaptação baixa rende um pouco menos (até -15% de
//! força) nas duas primeiras temporadas depois de uma transferência
//! ([`adaptation::apply_adaptation_penalty`]) — malus puro, nunca bônus,
//! e some depois da janela. Com isto, os oito atributos ocultos de
//! RF-JG-02 têm todos uma fatia real implementada.
#![warn(clippy::all)]

mod adaptation;
mod condition;
mod consistency;
mod contract;
mod cup;
mod discipline;
mod finance;
mod fixture_key;
mod injuries;
mod market;
mod morale;
mod progression;
mod quality;
mod reputation;
mod season;
mod strength;

pub use adaptation::{
    advance_seasons_since_transfer, generate_adaptation, initial_seasons_since_transfer,
    mark_transferred,
};
pub use condition::{FULL_CONDITION, apply_season_fatigue};
pub use consistency::generate_consistency;
pub use contract::{ContractState, advance_contracts, generate_initial_contracts};
pub use cup::{CupMatch, CupResult, cup_participants, generate_big_game_mentality, run_cup};
pub use discipline::generate_dirtiness;
pub use finance::{
    distribute_prize_money, distribute_tv_revenue, generate_budgets, generate_stadium_capacities,
    pay_salaries, pay_sponsorship, repay_debt, salary_per_season,
};
pub use injuries::{generate_injury_proneness, roll_injuries};
pub use market::{generate_ambition, generate_loyalty, run_market_day};
pub use progression::{PlayerState, advance_season, generate_professionalism, initial_roster};
pub use quality::{
    generate_match_profiles, generate_match_profiles_from_roster, starters_from_roster_all_clubs,
};
pub use reputation::generate_club_reputation;
pub use season::{CareerRound, SeasonResult, run_season, run_seasons};
pub use strength::{generate_strengths, generate_strengths_from_roster, strength_of};
