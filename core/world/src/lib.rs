//! `world` — o loop que faz o mundo girar sozinho, sem UI: calendário,
//! temporada, tabela, promoção e rebaixamento, progressão de jogadores,
//! lesões e mercado. Consome um `pack::LoadedPack`, o `engine` de partida e
//! a `ai` de clube; é o portão do M1
//! (`docs/07-roadmap.md#m1--kick-off-headless-10-semanas`).
//!
//! Ainda não implementado: regens, aposentadoria, moral, calendário mensal
//! (`docs/07-roadmap.md`). Clubes sem elenco no pack usam força e
//! orçamento sintéticos ([`strength`], [`finance`]), substitutos
//! documentados, não uma simulação de elenco. Copa ([`cup`]) é fase única,
//! sem sorteio de confronto e com empate decidido por moeda — escopo mais
//! estreito que uma copa de verdade, documentado em [`cup`]. Condição
//! ([`condition`]) é por temporada, não cumulativa e não cobre fadiga
//! acumulada de carreira nem frescor dia a dia — documentado em
//! [`condition`]. Suspensão ([`discipline`]) é a única coisa aqui com
//! granularidade de rodada (liga simulada rodada a rodada no caminho de
//! carreira, `season::run_season`) — sem cartão amarelo acumulado, sem
//! reincidência, documentado em [`discipline`]. Folha salarial
//! ([`finance::pay_salaries`]) é a primeira despesa recorrente de verdade
//! — fatia mínima de RF-CL-02, sem contrato, duração, luvas ou cláusula.
//! Bilheteria ([`finance::match_day_revenue`]) é a primeira receita
//! recorrente de verdade — fatia mínima de RF-CL-03, só ingresso de
//! partida em casa, sem TV, prêmio ou patrocínio. Duração de contrato
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
//! sem nenhum efeito na liga.
#![warn(clippy::all)]

mod condition;
mod contract;
mod cup;
mod discipline;
mod finance;
mod fixture_key;
mod injuries;
mod market;
mod progression;
mod quality;
mod reputation;
mod season;
mod strength;

pub use condition::{FULL_CONDITION, apply_season_fatigue};
pub use contract::{ContractState, advance_contracts, generate_initial_contracts};
pub use cup::{CupMatch, CupResult, cup_participants, generate_big_game_mentality, run_cup};
pub use discipline::generate_dirtiness;
pub use finance::{generate_budgets, generate_stadium_capacities, pay_salaries, salary_per_season};
pub use injuries::{generate_injury_proneness, roll_injuries};
pub use market::{generate_loyalty, run_market_day};
pub use progression::{PlayerState, advance_season, generate_professionalism, initial_roster};
pub use quality::{
    generate_match_profiles, generate_match_profiles_from_roster, starters_from_roster_all_clubs,
};
pub use reputation::generate_club_reputation;
pub use season::{CareerRound, SeasonResult, run_season, run_seasons};
pub use strength::{generate_strengths, generate_strengths_from_roster, strength_of};
