//! Moral — fatia mínima de RF-JG-07 (`docs/01 §2.1`: "Moral, condição
//! física, fadiga acumulada e frescor de jogo"), ao lado da condição
//! física que `crate::condition` já cobre. Simplificação deliberada: moral
//! aqui é **por clube**, não por jogador — o saldo de vitórias/derrotas
//! nas últimas partidas de liga já disputadas na temporada, não uma
//! reação individual a minutos jogados, decisão do técnico ou resultado
//! de outro jogo (isso pediria o sistema de personalidade de RF-JG-10,
//! M4). Time embalado por vitórias joga um pouco melhor; time em sequência
//! de derrotas joga um pouco pior — o "efeito manada" clássico de qualquer
//! manager de futebol, sem nenhuma das causas mais finas de verdade ainda.
//!
//! [`apply_morale`] é uma função pura: não sorteia nada, só lê
//! `RoundRobinOutcome::results` (`crate::season`, seu único consumidor) —
//! por isso não precisa de `world_seed` nem de estado de carreira próprio
//! (ao contrário de `crate::adaptation`, que precisa saber "há quantas
//! temporadas desde a transferência"). Moral nunca atravessa temporada:
//! `results_so_far` é sempre só o que já foi simulado **nesta** temporada
//! (reinicia vazio a cada uma, mesma disciplina de
//! `crate::condition`/`crate::injuries`/`crate::discipline`). Só existe no
//! caminho de carreira (o caminho estático de `managerfc-cli
//! calibrate`/`bench` simula a temporada inteira de uma vez, sem nenhum
//! ponto intermediário onde "o que já aconteceu nesta temporada" exista) e
//! não afeta copa (mesma exclusão de todo atributo com granularidade de
//! rodada, `crate::cup`/`crate::consistency`/`crate::adaptation`).

use std::cmp::Ordering;

use domain::{ClubId, Fixed};
use engine::TeamMatchProfile;
use rules::{Fixture, Score};

/// Quantas partidas de liga recentes entram no cálculo de moral — só a
/// forma mais recente, não a temporada inteira.
const MORALE_WINDOW: usize = 3;

/// Efeito de moral por vitória/derrota líquida no saldo das últimas
/// [`MORALE_WINDOW`] partidas, em permilagem — de -9% (perdeu todas as
/// 3) a +9% (venceu todas as 3); empate não conta pra nenhum lado. Não
/// calibrado, só a direção certa.
const MORALE_STEP_PERMILLE: i64 = 30;

/// `1` se `club` venceu essa partida, `-1` se perdeu, `0` se empatou —
/// `None` se `club` não jogou essa partida (não é mandante nem visitante).
fn net_result(fixture: &Fixture, score: &Score, club: ClubId) -> Option<i64> {
    let goal_difference = if fixture.home == club {
        i64::from(score.home_goals) - i64::from(score.away_goals)
    } else if fixture.away == club {
        i64::from(score.away_goals) - i64::from(score.home_goals)
    } else {
        return None;
    };
    Some(match goal_difference.cmp(&0) {
        Ordering::Greater => 1,
        Ordering::Equal => 0,
        Ordering::Less => -1,
    })
}

/// Escala a força de `profile` conforme a moral recente de `club` — o
/// saldo de vitórias/derrotas nas últimas [`MORALE_WINDOW`] partidas de
/// liga de `club` dentro de `results_so_far` (`crate::season`'s
/// `RoundRobinOutcome::results`, na ordem em que foram simuladas: as mais
/// recentes primeiro, não por data — não há calendário diário). Sem
/// nenhuma partida de `club` ainda em `results_so_far` (começo da
/// temporada), devolve `profile` sem mudança — moral neutra é a mesma
/// força de antes desta fatia.
#[must_use]
pub fn apply_morale(
    profile: TeamMatchProfile,
    club: ClubId,
    results_so_far: &[(Fixture, Score)],
) -> TeamMatchProfile {
    let net: i64 = results_so_far
        .iter()
        .rev()
        .filter_map(|(fixture, score)| net_result(fixture, score, club))
        .take(MORALE_WINDOW)
        .sum();
    if net == 0 {
        return profile;
    }
    let multiplier = Fixed::from_ratio(1000 + net * MORALE_STEP_PERMILLE, 1000);
    TeamMatchProfile {
        strength: engine::TeamStrength::new(profile.strength.value() * multiplier),
        ..profile
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::{FinishingQuality, GoalkeepingQuality, TeamStrength};

    fn profile(strength: i32) -> TeamMatchProfile {
        TeamMatchProfile {
            strength: TeamStrength::new(Fixed::from_int(strength)),
            finishing: FinishingQuality::new(Fixed::from_int(10)),
            goalkeeping: GoalkeepingQuality::new(Fixed::from_int(10)),
        }
    }

    fn win(round: u32, home: ClubId, away: ClubId) -> (Fixture, Score) {
        (
            Fixture { round, home, away },
            Score {
                home_goals: 2,
                away_goals: 0,
            },
        )
    }

    fn loss(round: u32, home: ClubId, away: ClubId) -> (Fixture, Score) {
        (
            Fixture { round, home, away },
            Score {
                home_goals: 0,
                away_goals: 2,
            },
        )
    }

    fn draw(round: u32, home: ClubId, away: ClubId) -> (Fixture, Score) {
        (
            Fixture { round, home, away },
            Score {
                home_goals: 1,
                away_goals: 1,
            },
        )
    }

    #[test]
    fn sem_historico_nesta_temporada_nao_muda_nada() {
        let base = profile(300);
        let result = apply_morale(base, ClubId::new(0), &[]);
        assert_eq!(result.strength.value(), base.strength.value());
    }

    #[test]
    fn tres_vitorias_seguidas_aumenta_a_forca() {
        let club = ClubId::new(0);
        let rival = ClubId::new(1);
        let results = vec![
            win(0, club, rival),
            win(1, club, rival),
            win(2, club, rival),
        ];
        let base = profile(300);
        let result = apply_morale(base, club, &results);
        assert!(result.strength.value() > base.strength.value());
    }

    #[test]
    fn conta_tanto_como_mandante_quanto_como_visitante() {
        // `club` vence uma vez em casa e uma vez fora — as duas deveriam
        // contar a favor (a derrota de `rival`, do lado de `rival`, é a
        // mesma partida vista pelo outro lado).
        let club = ClubId::new(0);
        let rival = ClubId::new(1);
        let results = vec![
            win(0, club, rival),  // club em casa, vence
            loss(1, rival, club), // club fora (visitante vence), rival em casa perde
        ];
        let base = profile(300);
        let result = apply_morale(base, club, &results);
        assert!(result.strength.value() > base.strength.value());
    }

    #[test]
    fn tres_derrotas_seguidas_reduz_a_forca() {
        let club = ClubId::new(0);
        let rival = ClubId::new(1);
        let results = vec![
            loss(0, club, rival),
            loss(1, club, rival),
            loss(2, club, rival),
        ];
        let base = profile(300);
        let result = apply_morale(base, club, &results);
        assert!(result.strength.value() < base.strength.value());
    }

    #[test]
    fn empates_nao_mudam_a_moral() {
        let club = ClubId::new(0);
        let rival = ClubId::new(1);
        let results = vec![draw(0, club, rival), draw(1, rival, club)];
        let base = profile(300);
        let result = apply_morale(base, club, &results);
        assert_eq!(result.strength.value(), base.strength.value());
    }

    #[test]
    fn so_considera_as_ultimas_tres_partidas() {
        // 3 vitórias seguidas de 3 derrotas: só as 3 derrotas mais
        // recentes deveriam contar — efeito líquido negativo, não neutro.
        let club = ClubId::new(0);
        let rival = ClubId::new(1);
        let results = vec![
            win(0, club, rival),
            win(1, club, rival),
            win(2, club, rival),
            loss(3, club, rival),
            loss(4, club, rival),
            loss(5, club, rival),
        ];
        let base = profile(300);
        let result = apply_morale(base, club, &results);
        assert!(result.strength.value() < base.strength.value());
    }

    #[test]
    fn so_conta_partida_de_quem_jogou() {
        // Resultado de outro confronto (nem `club` nem `rival` envolvidos)
        // não deveria influenciar a moral de `club`.
        let club = ClubId::new(0);
        let rival = ClubId::new(1);
        let outros = (ClubId::new(2), ClubId::new(3));
        let results = vec![
            win(0, club, rival),
            loss(1, outros.0, outros.1),
            loss(2, outros.1, outros.0),
        ];
        let base = profile(300);
        let result = apply_morale(base, club, &results);
        assert!(
            result.strength.value() > base.strength.value(),
            "vitória de `club` deveria pesar, resultado de outro confronto não deveria"
        );
    }

    #[test]
    fn mesma_vitoria_e_derrota_isoladas_tem_efeito_simetrico() {
        let club = ClubId::new(0);
        let rival = ClubId::new(1);
        let base = profile(300);

        let after_win = apply_morale(base, club, &[win(0, club, rival)]);
        let after_loss = apply_morale(base, club, &[loss(0, club, rival)]);

        let win_delta = after_win.strength.value() - base.strength.value();
        let loss_delta = base.strength.value() - after_loss.strength.value();
        assert_eq!(win_delta, loss_delta);
    }
}
