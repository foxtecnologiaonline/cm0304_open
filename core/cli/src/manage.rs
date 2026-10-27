//! `managerfc-cli manage` — cliente de texto do ciclo jogável: assumir um
//! clube, escalar, treinar, negociar, jogar rodada a rodada ou uma partida
//! ao vivo, ler a caixa de entrada e salvar. Existe para **provar de fora do
//! crate `app`** que a fronteira `dispatch`/`query` basta para jogar de
//! verdade (a UI Flutter, que consumirá a mesma fronteira, não dá para
//! construir nem verificar sem o SDK Dart) e como roteiro de regressão:
//! `--script` roda um arquivo de comandos e sai com erro se algum falhar
//! (`--strict`).
//!
//! Tudo aqui é apresentação: nenhuma regra de jogo mora neste módulo — ele só
//! chama `GameSession::dispatch`/`query` e formata. O texto é em português
//! porque é uma ferramenta de desenvolvimento; o produto localiza na UI a
//! partir dos dados estruturados do núcleo.

use std::fmt::Write as _;
use std::io::{self, BufRead, Write};

use app::{
    AppError, Command, CommandReceipt, Formation, GameSession, InboxKind, Mentality, OfferOutcome,
    PlayerFilter, Pressing, Query, QueryResult, Refusal, Side, Training,
};
use domain::{ClubId, Money, PlayerId, Position};

/// Resposta a uma linha de comando.
#[derive(Debug, PartialEq, Eq)]
pub enum Reply {
    Text(String),
    Quit,
}

/// Estado do cliente além da sessão: até onde o relato ao vivo já foi
/// mostrado.
#[derive(Debug, Default)]
pub struct ClientState {
    live_cursor: usize,
}

const HELP: &str = "\
comandos:
  status                         resumo do clube, da temporada e da janela
  elenco                         jogadores do seu clube (id, posição, CA, ...)
  jogador ID                     ficha de um jogador
  escalacao                      onze, formação e instruções
  formacao 4-4-2|4-3-3|3-5-2|4-5-1|5-3-2   formação com escalação automática
  time ID x11                    escalação manual (na ordem dos espaços)
  instrucoes defensiva|equilibrada|ofensiva normal|alta
  treino ID leve|normal|intenso  treino de um jogador
  treino-time leve|normal|intenso
  calendario                     seus jogos da temporada
  tabela                         classificação das ligas
  rodada [N]                     resultados da rodada N (padrão: a última)
  iniciar                        pré-temporada (abre a janela)
  proximo                        joga uma rodada
  temporada                      joga o resto da temporada
  ao-vivo                        começa sua próxima partida ao vivo
  jogar N                        joga N minutos da partida ao vivo
  trocar SAI ENTRA               substituição ao vivo
  relato                         mostra o relato novo da partida ao vivo
  ultima                         relatório da última partida
  janela                         estado da janela de transferências
  buscar [GOL|DEF|MEI|ATA] [min-CA] [max-idade]   jogadores de outros clubes
  comprar ID TAXA SALARIO% ANOS  proposta (ex.: comprar 40 1500000.00 120 3)
  renovar ID SALARIO% ANOS       renova contrato de um jogador seu
  propostas                      propostas da IA pelos seus jogadores
  responder ID sim|nao           responde a uma proposta
  caixa [nao-lidas]              caixa de entrada
  lida ID | lidas                marca como lida
  salvar CAMINHO                 grava a carreira
  ajuda | sair";

fn pos_label(p: Position) -> &'static str {
    match p {
        Position::Goalkeeper => "GOL",
        Position::Defender => "DEF",
        Position::Midfielder => "MEI",
        Position::Forward => "ATA",
    }
}

fn parse_position(s: &str) -> Option<Position> {
    match s.to_lowercase().as_str() {
        "gol" => Some(Position::Goalkeeper),
        "def" => Some(Position::Defender),
        "mei" => Some(Position::Midfielder),
        "ata" => Some(Position::Forward),
        _ => None,
    }
}

fn training_label(t: Training) -> &'static str {
    match t {
        Training::Light => "leve",
        Training::Normal => "normal",
        Training::Intense => "intenso",
    }
}

fn parse_training(s: &str) -> Option<Training> {
    match s.to_lowercase().as_str() {
        "leve" => Some(Training::Light),
        "normal" => Some(Training::Normal),
        "intenso" => Some(Training::Intense),
        _ => None,
    }
}

/// `"1234.56"` → centavos, sem ponto flutuante. Aceita até 2 casas decimais.
fn parse_money(s: &str) -> Option<Money> {
    let (whole, frac) = s.split_once('.').unwrap_or((s, ""));
    if whole.is_empty() || frac.len() > 2 || !whole.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if !frac.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let cents_part: i64 = format!("{frac:0<2}").parse().ok()?;
    let cents = whole
        .parse::<i64>()
        .ok()?
        .checked_mul(100)?
        .checked_add(cents_part)?;
    Some(Money::from_cents(cents))
}

fn num<T: std::str::FromStr>(token: Option<&str>, what: &str) -> Result<T, String> {
    token
        .ok_or_else(|| format!("falta {what}"))?
        .parse()
        .map_err(|_| format!("{what} inválido"))
}

fn err(e: AppError) -> String {
    e.to_string()
}

fn offer_text(outcome: OfferOutcome) -> String {
    match outcome {
        OfferOutcome::Accepted => "FECHADO.".to_string(),
        OfferOutcome::ClubWantsMore { asking } => {
            format!("O clube quer mais: pede {asking} para fechar.")
        }
        OfferOutcome::ClubRefuses(r) => format!("O clube recusa: {}.", refusal_text(r)),
        OfferOutcome::CannotAfford => "Você não tem caixa para essa taxa.".to_string(),
        OfferOutcome::PlayerWantsMore {
            asking_wage_permille,
        } => format!(
            "O jogador quer mais salário: pede {}% do salário-base.",
            asking_wage_permille / 10
        ),
        OfferOutcome::PlayerRefuses(r) => format!("O jogador recusa: {}.", refusal_text(r)),
    }
}

fn refusal_text(r: Refusal) -> &'static str {
    match r {
        Refusal::InsultingOffer => "proposta ofensiva",
        Refusal::NotForSale => "jogador intocável",
        Refusal::SquadLimits => "venderia o elenco abaixo do mínimo",
        Refusal::WantsBiggerClub => "quer um clube maior",
    }
}

struct View<'a> {
    session: &'a GameSession,
}

impl View<'_> {
    fn q(&self, query: Query) -> QueryResult {
        self.session.query(query)
    }

    fn manager(&self) -> Result<(app::ManagerInfo, ClubId), String> {
        match self.q(Query::Manager) {
            QueryResult::Manager(Some(m)) => {
                let club = m.club;
                Ok((m, club))
            }
            _ => Err("nenhum clube sob comando".to_string()),
        }
    }

    fn player_name(&self, id: PlayerId) -> String {
        match self.q(Query::Player { player: id }) {
            QueryResult::Player(Some(d)) => d.row.name,
            _ => format!("#{}", id.index()),
        }
    }

    fn club_name(&self, club: ClubId) -> String {
        match self.q(Query::Club { club }) {
            QueryResult::Club(Some(c)) => c.name,
            _ => format!("clube#{}", club.index()),
        }
    }

    fn progress(&self) -> app::SeasonProgress {
        match self.q(Query::SeasonProgress) {
            QueryResult::SeasonProgress(p) => p,
            _ => unreachable!("SeasonProgress responde SeasonProgress"),
        }
    }
}

fn status(session: &GameSession) -> Result<String, String> {
    let v = View { session };
    let (m, club) = v.manager()?;
    let QueryResult::Club(Some(c)) = v.q(Query::Club { club }) else {
        return Err("clube inexistente".to_string());
    };
    let p = v.progress();
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{} — {} ({}) — técnico {}",
        c.name, c.competition_name, c.nation_name, m.coach_name
    );
    if p.in_progress {
        let _ = writeln!(
            out,
            "Temporada {}: próxima rodada {} de {}",
            p.season_index + 1,
            p.next_matchday + 1,
            p.total_matchdays
        );
    } else {
        let _ = writeln!(
            out,
            "Temporada {}: pré-temporada (use `iniciar`)",
            p.season_index + 1
        );
    }
    let _ = writeln!(
        out,
        "Caixa {} | dívida {} | elenco {} | folha/temporada {}",
        c.budget, c.debt, c.squad_size, c.payroll_per_season
    );
    if let QueryResult::TransferWindow(w) = v.q(Query::TransferWindow) {
        let _ = match (w.open, w.matchdays_until_change) {
            (true, Some(n)) => writeln!(
                out,
                "Janela de transferências: ABERTA (fecha em {n} rodada(s))"
            ),
            (true, None) => writeln!(out, "Janela de transferências: ABERTA"),
            (false, Some(n)) => writeln!(
                out,
                "Janela de transferências: fechada (abre em {n} rodada(s))"
            ),
            (false, None) => writeln!(out, "Janela de transferências: fechada"),
        };
    }
    if let QueryResult::Inbox(page) = v.q(Query::Inbox {
        unread_only: true,
        offset: 0,
        limit: 1,
    }) {
        let _ = writeln!(out, "Caixa de entrada: {} não lida(s)", page.unread);
    }
    if let QueryResult::LiveMatch(Some(live)) = v.q(Query::LiveMatch { lines_from: 0 }) {
        let _ = writeln!(
            out,
            "Partida ao vivo: {} {} x {} {} — {}'{}",
            live.home_name,
            live.home_goals,
            live.away_goals,
            live.away_name,
            live.minute,
            if live.finished { " (encerrada)" } else { "" }
        );
    }
    Ok(out.trim_end().to_string())
}

fn squad(session: &GameSession) -> Result<String, String> {
    let v = View { session };
    let (_, club) = v.manager()?;
    let QueryResult::Squad(Some(rows)) = v.q(Query::Squad { club }) else {
        return Err("clube inexistente".to_string());
    };
    let mut out = String::from(
        " id  pos nome                  idade  CA cond  situação  contrato salário/temp  valor      treino\n",
    );
    for r in rows {
        let status = if r.injured {
            "LESÃO"
        } else if r.suspended {
            "SUSP"
        } else {
            "ok"
        };
        let _ = writeln!(
            out,
            "{:>3}  {} {:<22}{:>4}  {:>3} {:>4}  {:<8}  {}a {:>13}  {:>10} {}",
            r.id.index(),
            pos_label(r.position),
            r.name,
            r.age,
            r.current_ability,
            r.condition,
            status,
            r.contract_seasons_remaining,
            r.salary_per_season,
            r.market_value,
            training_label(r.training),
        );
    }
    Ok(out.trim_end().to_string())
}

fn player(session: &GameSession, token: Option<&str>) -> Result<String, String> {
    let id = PlayerId::new(num(token, "o id do jogador")?);
    let QueryResult::Player(Some(d)) = session.query(Query::Player { player: id }) else {
        return Err("jogador inexistente".to_string());
    };
    let r = &d.row;
    let mut out = format!(
        "{} — {} ({}), {} anos, {}\nCA {} | condição {} | contrato {}a, salário {} ({}%) | valor {}\n",
        r.name,
        d.club_name,
        pos_label(r.position),
        r.age,
        d.nation_name,
        r.current_ability,
        r.condition,
        r.contract_seasons_remaining,
        r.salary_per_season,
        r.wage_permille / 10,
        r.market_value,
    );
    for chunk in d.attributes.chunks(6) {
        let line: Vec<String> = chunk
            .iter()
            .map(|a| format!("{}={}", a.attribute.key(), a.value))
            .collect();
        let _ = writeln!(out, "  {}", line.join("  "));
    }
    Ok(out.trim_end().to_string())
}

fn lineup(session: &GameSession) -> Result<String, String> {
    let QueryResult::Lineup(Some(l)) = session.query(Query::Lineup) else {
        return Err("nenhum clube sob comando".to_string());
    };
    let mut out = format!(
        "Formação {} ({}) | mentalidade {:?} | pressão {:?}\n",
        l.formation.key(),
        if l.manual { "manual" } else { "automática" },
        l.mentality,
        l.pressing
    );
    for s in &l.slots {
        let note = if s.fit_permille < 1000 {
            format!("  (fora de posição: {}%)", s.fit_permille / 10)
        } else {
            String::new()
        };
        let _ = writeln!(
            out,
            "  {} {:>3} {:<22} cond {}{}",
            pos_label(s.slot_position),
            s.player.index(),
            s.name,
            s.condition,
            note
        );
    }
    for (out_id, in_id) in &l.adjustments {
        let _ = writeln!(
            out,
            "  ! {} indisponível: entra {}",
            out_id.index(),
            in_id.index()
        );
    }
    Ok(out.trim_end().to_string())
}

fn standings(session: &GameSession) -> Result<String, String> {
    let QueryResult::Competitions(list) = session.query(Query::Competitions) else {
        return Err("sem competições".to_string());
    };
    let mut out = String::new();
    for comp in list {
        if let QueryResult::Standings(Some(rows)) = session.query(Query::Standings {
            competition: comp.id,
        }) {
            let _ = writeln!(out, "== {} ==", comp.name);
            for r in rows {
                let _ = writeln!(
                    out,
                    "{:>2}. {:<24} J{:>3} V{:>3} E{:>3} D{:>3} {:>3}:{:<3} {:>3} pts",
                    r.position,
                    r.club_name,
                    r.played,
                    r.wins,
                    r.draws,
                    r.losses,
                    r.goals_for,
                    r.goals_against,
                    r.points
                );
            }
        }
    }
    if out.is_empty() {
        out.push_str("Nenhuma temporada concluída ainda.");
    }
    Ok(out.trim_end().to_string())
}

fn fixtures(session: &GameSession) -> Result<String, String> {
    let v = View { session };
    let (_, club) = v.manager()?;
    let QueryResult::Fixtures(Some(list)) = v.q(Query::Fixtures { club }) else {
        return Err("o calendário só existe depois de `iniciar`".to_string());
    };
    let mut out = String::new();
    for r in list.rows {
        let score = r.score.map_or("   -   ".to_string(), |s| {
            format!(" {} x {} ", s.home_goals, s.away_goals)
        });
        let _ = writeln!(
            out,
            "R{:>2}  {:<22}{}{}",
            r.matchday + 1,
            r.home_name,
            score,
            r.away_name
        );
    }
    Ok(out.trim_end().to_string())
}

fn round(session: &GameSession, token: Option<&str>) -> Result<String, String> {
    let matchday = match token {
        Some(t) => Some(num::<u32>(Some(t), "a rodada")?.saturating_sub(1)),
        None => None,
    };
    let QueryResult::RoundResults(Some(r)) = session.query(Query::RoundResults { matchday }) else {
        return Err("rodada ainda não jogada".to_string());
    };
    let mut out = format!("Rodada {}:\n", r.matchday + 1);
    for row in r.rows {
        let _ = writeln!(
            out,
            "  {:<22} {} x {} {}",
            row.home_name, row.score.home_goals, row.score.away_goals, row.away_name
        );
    }
    Ok(out.trim_end().to_string())
}

fn line_text(home: &str, away: &str, l: &app::MatchLineRow) -> String {
    let team = |s: Option<Side>| match s {
        Some(Side::Home) => home.to_string(),
        Some(Side::Away) => away.to_string(),
        None => String::new(),
    };
    let name = |p: &Option<app::PlayerRef>| p.as_ref().map_or("?".to_string(), |p| p.name.clone());
    match l.kind {
        app::MatchLineKind::KickOff => "Começa a partida.".to_string(),
        app::MatchLineKind::HalfTime => format!(
            "Intervalo: {} {} x {} {}",
            home, l.home_goals, l.away_goals, away
        ),
        app::MatchLineKind::FullTime => format!(
            "Fim de jogo: {} {} x {} {}",
            home, l.home_goals, l.away_goals, away
        ),
        app::MatchLineKind::Shot => format!(
            "{}' {} finaliza ({}).",
            l.minute,
            name(&l.player),
            team(l.side)
        ),
        app::MatchLineKind::Goal => {
            let assist = l
                .other
                .as_ref()
                .map_or(String::new(), |a| format!(" (assist.: {})", a.name));
            format!(
                "{}' GOL! {} ({}){} — {} {} x {} {}",
                l.minute,
                name(&l.player),
                team(l.side),
                assist,
                home,
                l.home_goals,
                l.away_goals,
                away
            )
        }
        app::MatchLineKind::Substitution => format!(
            "{}' Substituição ({}): entra {}, sai {}.",
            l.minute,
            team(l.side),
            name(&l.player),
            name(&l.other)
        ),
    }
}

fn live_report(session: &GameSession, state: &mut ClientState) -> Result<String, String> {
    let QueryResult::LiveMatch(Some(view)) = session.query(Query::LiveMatch {
        lines_from: state.live_cursor,
    }) else {
        return Err("nenhuma partida ao vivo".to_string());
    };
    state.live_cursor = view.total_lines;
    let mut out = String::new();
    for l in &view.lines {
        let _ = writeln!(out, "{}", line_text(&view.home_name, &view.away_name, l));
    }
    let _ = write!(
        out,
        "[{}'  {} {} x {} {}  | finalizações {}-{} | trocas restantes {}]",
        view.minute,
        view.home_name,
        view.home_goals,
        view.away_goals,
        view.away_name,
        view.home_shots,
        view.away_shots,
        view.substitutions_left
    );
    Ok(out)
}

fn last_match(session: &GameSession) -> Result<String, String> {
    let QueryResult::LastMatch(Some(m)) = session.query(Query::LastMatch) else {
        return Err("nenhuma partida jogada ainda".to_string());
    };
    let mut out = format!(
        "{} — temporada {}\n{} {} x {} {}  | finalizações {}-{} | xG {:.2}-{:.2}\n",
        m.competition_name,
        m.season_index + 1,
        m.home_name,
        m.home_goals,
        m.away_goals,
        m.away_name,
        m.home_shots,
        m.away_shots,
        f64::from(m.home_xg_milli) / 1000.0,
        f64::from(m.away_xg_milli) / 1000.0,
    );
    for l in &m.lines {
        let _ = writeln!(out, "  {}", line_text(&m.home_name, &m.away_name, l));
    }
    let mut best: Vec<_> = m.players.iter().collect();
    best.sort_by_key(|p| std::cmp::Reverse(p.rating_x10));
    out.push_str("Melhores notas:\n");
    for p in best.into_iter().take(3) {
        let _ = writeln!(
            out,
            "  {:<22} {}.{}",
            p.player.name,
            p.rating_x10 / 10,
            p.rating_x10 % 10
        );
    }
    Ok(out.trim_end().to_string())
}

fn search(session: &GameSession, tokens: &[&str]) -> Result<String, String> {
    let v = View { session };
    let (_, club) = v.manager()?;
    let mut filter = PlayerFilter {
        not_from_club: Some(club),
        ..PlayerFilter::default()
    };
    for t in tokens {
        if let Some(p) = parse_position(t) {
            filter.position = Some(p);
        } else if let Ok(n) = t.parse::<u8>() {
            // Primeiro número = CA mínimo; segundo = idade máxima.
            if filter.min_ability == 0 {
                filter.min_ability = n;
            } else {
                filter.max_age = Some(n);
            }
        } else {
            return Err(format!("filtro desconhecido: {t}"));
        }
    }
    let QueryResult::Search(page) = v.q(Query::Search {
        filter,
        offset: 0,
        limit: 15,
    }) else {
        return Err("busca indisponível".to_string());
    };
    let mut out = format!(
        "{} jogador(es); mostrando {}:\n",
        page.total,
        page.rows.len()
    );
    for r in page.rows {
        let _ = writeln!(
            out,
            "{:>4}  {} {:<22}{:>3}a CA {:>3}  valor {:>12}  {}",
            r.id.index(),
            pos_label(r.position),
            r.name,
            r.age,
            r.current_ability,
            r.market_value,
            v.club_name(r.club)
        );
    }
    Ok(out.trim_end().to_string())
}

fn offers(session: &GameSession) -> Result<String, String> {
    let QueryResult::IncomingOffers(rows) = session.query(Query::IncomingOffers) else {
        return Err("sem propostas".to_string());
    };
    if rows.is_empty() {
        return Ok("Nenhuma proposta no momento.".to_string());
    }
    let mut out = String::new();
    for o in rows {
        let _ = writeln!(
            out,
            "proposta {}: {} ({} CA {}) — {} oferece {} (valor {}) — expira em {} rodada(s)",
            o.id,
            o.player.name,
            pos_label(o.position),
            o.current_ability,
            o.from_name,
            o.fee,
            o.market_value,
            o.expires_in_matchdays
        );
    }
    Ok(out.trim_end().to_string())
}

fn inbox_kind_text(session: &GameSession, kind: InboxKind) -> String {
    let v = View { session };
    match kind {
        InboxKind::SeasonStarted => "Pré-temporada concluída.".to_string(),
        InboxKind::MatchPlayed {
            home,
            away,
            home_goals,
            away_goals,
        } => format!(
            "Resultado: {} {} x {} {}",
            v.club_name(home),
            home_goals,
            away_goals,
            v.club_name(away)
        ),
        InboxKind::PlayersInjured { count } => format!("{count} jogador(es) seu(s) lesionado(s)."),
        InboxKind::PlayerSuspended { player } => {
            format!("{} está suspenso na próxima rodada.", v.player_name(player))
        }
        InboxKind::LineupAdjusted { out, incoming } => format!(
            "{} indisponível; {} entrou no lugar.",
            v.player_name(out),
            v.player_name(incoming)
        ),
        InboxKind::OfferReceived {
            offer,
            player,
            from,
            fee,
        } => format!(
            "Proposta {offer}: {} oferece {fee} por {}.",
            v.club_name(from),
            v.player_name(player)
        ),
        InboxKind::ContractsExpiring { count } => {
            format!("{count} jogador(es) com contrato no último ano.")
        }
        InboxKind::TransferWindowOpened => "Janela de transferências aberta.".to_string(),
        InboxKind::TransferWindowClosed => "Janela de transferências fechada.".to_string(),
        InboxKind::DebtWarning { debt } => format!("Atenção: dívida de {debt}."),
        InboxKind::SeasonEnded {
            position,
            clubs,
            promoted,
            relegated,
        } => format!(
            "Fim de temporada: {position}º de {clubs}{}.",
            if promoted {
                " — ACESSO!"
            } else if relegated {
                " — REBAIXADO."
            } else {
                ""
            }
        ),
    }
}

fn inbox(session: &GameSession, unread_only: bool) -> Result<String, String> {
    let QueryResult::Inbox(page) = session.query(Query::Inbox {
        unread_only,
        offset: 0,
        limit: 12,
    }) else {
        return Err("caixa indisponível".to_string());
    };
    let mut out = format!(
        "{} mensagem(ns), {} não lida(s):\n",
        page.total, page.unread
    );
    for m in page.rows {
        let _ = writeln!(
            out,
            "{} #{:<3} T{} R{:>2}  {}{}",
            if m.read { " " } else { "*" },
            m.id,
            m.season_index + 1,
            m.matchday,
            inbox_kind_text(session, m.kind),
            if m.kind.needs_action() {
                "  <- decisão"
            } else {
                ""
            }
        );
    }
    Ok(out.trim_end().to_string())
}

fn receipt_summary(r: &CommandReceipt) -> String {
    let mut parts = Vec::new();
    if r.matches_played > 0 {
        parts.push(format!(
            "rodada {} de {}: {} partidas",
            r.matchday + 1,
            r.matchdays_total,
            r.matches_played
        ));
    }
    if r.season_completed {
        parts.push(format!(
            "temporada {} concluída ({} acesso/queda)",
            r.season_index + 1,
            r.movements
        ));
    }
    if r.lineup_adjustments > 0 {
        parts.push(format!("{} titular(es) trocado(s)", r.lineup_adjustments));
    }
    if let Some(o) = r.offer {
        parts.push(offer_text(o));
    }
    if parts.is_empty() {
        "ok".to_string()
    } else {
        parts.join("; ")
    }
}

fn dispatch(session: &mut GameSession, command: Command) -> Result<String, String> {
    session
        .dispatch(command)
        .map(|r| receipt_summary(&r))
        .map_err(err)
}

/// Executa uma linha de comando do cliente.
pub fn execute(
    session: &mut GameSession,
    state: &mut ClientState,
    line: &str,
) -> Result<Reply, String> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let Some((&cmd, args)) = tokens.split_first() else {
        return Ok(Reply::Text(String::new()));
    };
    let arg = |i: usize| args.get(i).copied();
    let text = match cmd {
        "ajuda" | "help" => HELP.to_string(),
        "sair" | "quit" => return Ok(Reply::Quit),
        "status" => status(session)?,
        "elenco" => squad(session)?,
        "jogador" => player(session, arg(0))?,
        "escalacao" => lineup(session)?,
        "calendario" => fixtures(session)?,
        "tabela" => standings(session)?,
        "rodada" => round(session, arg(0))?,
        "ultima" => last_match(session)?,
        "propostas" => offers(session)?,
        "caixa" => inbox(session, arg(0) == Some("nao-lidas"))?,
        "relato" => live_report(session, state)?,
        "buscar" => search(session, args)?,
        "janela" => match session.query(Query::TransferWindow) {
            QueryResult::TransferWindow(w) => format!(
                "Janela {}{}",
                if w.open { "ABERTA" } else { "fechada" },
                w.matchdays_until_change
                    .map_or(String::new(), |n| format!(" (muda em {n} rodada(s))"))
            ),
            _ => unreachable!("TransferWindow responde TransferWindow"),
        },
        "iniciar" => dispatch(session, Command::BeginSeason)?,
        "proximo" => {
            state.live_cursor = 0;
            dispatch(session, Command::AdvanceRound)?
        }
        "temporada" => {
            state.live_cursor = 0;
            dispatch(session, Command::AdvanceSeason)?
        }
        "ao-vivo" => {
            state.live_cursor = 0;
            dispatch(session, Command::StartMatch)?;
            live_report(session, state)?
        }
        "jogar" => {
            let minutes: u8 = num(arg(0), "os minutos")?;
            dispatch(session, Command::PlayMinutes { minutes })?;
            live_report(session, state)?
        }
        "trocar" => {
            let out = PlayerId::new(num(arg(0), "o jogador que sai")?);
            let incoming = PlayerId::new(num(arg(1), "o jogador que entra")?);
            dispatch(session, Command::Substitute { out, incoming })?;
            live_report(session, state)?
        }
        "formacao" => {
            let formation = Formation::ALL
                .into_iter()
                .find(|f| Some(f.key()) == arg(0))
                .ok_or("formação inválida (4-4-2, 4-3-3, 3-5-2, 4-5-1, 5-3-2)")?;
            dispatch(session, Command::SetFormation { formation })?;
            lineup(session)?
        }
        "time" => {
            let QueryResult::Lineup(Some(current)) = session.query(Query::Lineup) else {
                return Err("nenhum clube sob comando".to_string());
            };
            let starters = args
                .iter()
                .map(|t| {
                    t.parse::<u32>()
                        .map(PlayerId::new)
                        .map_err(|_| format!("id inválido: {t}"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            dispatch(
                session,
                Command::SetLineup {
                    formation: current.formation,
                    starters,
                },
            )?;
            lineup(session)?
        }
        "instrucoes" => {
            let mentality = match arg(0) {
                Some("defensiva") => Mentality::Defensive,
                Some("equilibrada") => Mentality::Balanced,
                Some("ofensiva") => Mentality::Attacking,
                _ => return Err("mentalidade: defensiva | equilibrada | ofensiva".to_string()),
            };
            let pressing = match arg(1) {
                Some("normal") | None => Pressing::Normal,
                Some("alta") => Pressing::High,
                _ => return Err("pressão: normal | alta".to_string()),
            };
            dispatch(
                session,
                Command::SetInstructions {
                    mentality,
                    pressing,
                },
            )?;
            lineup(session)?
        }
        "treino" => {
            let player = PlayerId::new(num(arg(0), "o jogador")?);
            let training = arg(1)
                .and_then(parse_training)
                .ok_or("treino: leve | normal | intenso")?;
            dispatch(session, Command::SetTraining { player, training })?
        }
        "treino-time" => {
            let training = arg(0)
                .and_then(parse_training)
                .ok_or("treino: leve | normal | intenso")?;
            dispatch(session, Command::SetTeamTraining { training })?
        }
        "comprar" => {
            let player = PlayerId::new(num(arg(0), "o jogador")?);
            let fee = arg(1)
                .and_then(parse_money)
                .ok_or("taxa inválida (ex.: 1500000.00)")?;
            let wage_percent: u16 = num(arg(2), "o salário em % do base")?;
            let seasons: u8 = num(arg(3), "os anos de contrato")?;
            dispatch(
                session,
                Command::MakeOffer {
                    player,
                    fee,
                    wage_permille: wage_percent.saturating_mul(10),
                    seasons,
                },
            )?
        }
        "renovar" => {
            let player = PlayerId::new(num(arg(0), "o jogador")?);
            let wage_percent: u16 = num(arg(1), "o salário em % do base")?;
            let seasons: u8 = num(arg(2), "os anos de contrato")?;
            dispatch(
                session,
                Command::RenewContract {
                    player,
                    wage_permille: wage_percent.saturating_mul(10),
                    seasons,
                },
            )?
        }
        "responder" => {
            let offer: u32 = num(arg(0), "a proposta")?;
            let accept = match arg(1) {
                Some("sim") => true,
                Some("nao") => false,
                _ => return Err("responda `sim` ou `nao`".to_string()),
            };
            dispatch(session, Command::RespondToOffer { offer, accept })?
        }
        "lida" => dispatch(
            session,
            Command::MarkInboxRead {
                id: num(arg(0), "a mensagem")?,
            },
        )?,
        "lidas" => dispatch(session, Command::MarkAllInboxRead)?,
        "salvar" => {
            let path = arg(0).ok_or("falta o caminho")?;
            session
                .save_to_path(std::path::Path::new(path))
                .map_err(err)?;
            format!("carreira gravada em {path}")
        }
        other => return Err(format!("comando desconhecido: {other} (use `ajuda`)")),
    };
    Ok(Reply::Text(text))
}

/// Laço de leitura: lê comandos de `input` até `sair` ou o fim, escrevendo
/// as respostas em `output`. Com `echo`, repete cada comando (útil em
/// roteiros). Com `strict`, o primeiro erro encerra com `Err`.
pub fn run(
    session: &mut GameSession,
    input: impl BufRead,
    output: &mut impl Write,
    echo: bool,
    strict: bool,
) -> io::Result<bool> {
    let mut state = ClientState::default();
    let mut all_ok = true;
    for line in input.lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if echo {
            writeln!(output, "> {trimmed}")?;
        }
        match execute(session, &mut state, trimmed) {
            Ok(Reply::Quit) => break,
            Ok(Reply::Text(text)) => {
                if !text.is_empty() {
                    writeln!(output, "{text}")?;
                }
            }
            Err(message) => {
                all_ok = false;
                writeln!(output, "erro: {message}")?;
                if strict {
                    return Ok(false);
                }
            }
        }
    }
    Ok(all_ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> GameSession {
        let mut s = GameSession::new(&crate::default_example_pack_path(), 42).unwrap();
        s.dispatch(Command::TakeClub {
            club: ClubId::new(3),
            coach: app::CoachProfile {
                name: "Zé".to_string(),
                nation: domain::NationId::new(0),
                reputation: 30,
            },
        })
        .unwrap();
        s
    }

    fn run_script(session: &mut GameSession, script: &str, strict: bool) -> (String, bool) {
        let mut out = Vec::new();
        let ok = run(session, script.as_bytes(), &mut out, false, strict).unwrap();
        (String::from_utf8(out).unwrap(), ok)
    }

    #[test]
    fn dinheiro_e_lido_sem_ponto_flutuante_e_rejeita_lixo() {
        assert_eq!(parse_money("1500.00"), Some(Money::from_cents(150_000)));
        assert_eq!(parse_money("12"), Some(Money::from_cents(1_200)));
        assert_eq!(parse_money("0.5"), Some(Money::from_cents(50)));
        assert_eq!(parse_money("7.05"), Some(Money::from_cents(705)));
        for bad in ["", ".5", "1.234", "-3", "abc", "1.2.3", "1,5"] {
            assert_eq!(parse_money(bad), None, "{bad:?}");
        }
        assert_eq!(
            parse_money("99999999999999999999"),
            None,
            "estouro vira None"
        );
    }

    #[test]
    fn roteiro_completo_joga_ao_vivo_rodada_temporada_e_salva() {
        let mut s = session();
        let path =
            std::env::temp_dir().join(format!("managerfc-manage-{}.sav", std::process::id()));
        let script = format!(
            "iniciar\nao-vivo\njogar 45\njogar 90\nproximo\nultima\ncaixa\ntemporada\ntabela\nsalvar {}\n",
            path.display()
        );
        let (out, ok) = run_script(&mut s, &script, true);
        assert!(ok, "{out}");
        assert!(out.contains("Começa a partida."));
        assert!(out.contains("Intervalo:"));
        assert!(out.contains("Fim de jogo:"));
        assert!(out.contains("temporada 1 concluída"));
        assert!(out.contains("== Primeira Divisão de Estrelônia =="));
        assert!(out.contains("Pré-temporada concluída."));
        assert!(out.contains("carreira gravada em"));

        // A carreira gravada reabre idêntica (replay do log de comandos).
        let mut reloaded =
            GameSession::load_from_path(&crate::default_example_pack_path(), &path).unwrap();
        let mut state = ClientState::default();
        let a = execute(&mut s, &mut state, "status").unwrap();
        let b = execute(&mut reloaded, &mut state, "status").unwrap();
        assert_eq!(a, b);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn mercado_pelo_cliente_oferta_recusada_nao_muda_nada_e_comando_invalido_e_erro() {
        let mut s = session();
        let (out, ok) = run_script(
            &mut s,
            "iniciar\ncomprar 222 1.00 100 3\nrenovar 49 50 2\ncomprar 99999 1.00 100 3\nfoo\n",
            false,
        );
        assert!(!ok);
        assert!(out.contains("proposta ofensiva"), "{out}");
        assert!(out.contains("quer mais salário"), "{out}");
        assert!(
            out.contains("erro: proposta inválida: jogador inexistente"),
            "{out}"
        );
        assert!(out.contains("erro: comando desconhecido: foo"), "{out}");
    }

    #[test]
    fn strict_para_no_primeiro_erro_e_comentarios_e_linhas_vazias_sao_ignorados() {
        let mut s = session();
        let (out, ok) = run_script(&mut s, "# comentário\n\nstatus\nfoo\nstatus\n", true);
        assert!(!ok);
        assert_eq!(
            out.matches("Baía Nova FC").count(),
            1,
            "o segundo status não roda: {out}"
        );
        let (all, ok_all) = run_script(&mut s, "foo\nstatus\n", false);
        assert!(!ok_all);
        assert!(all.contains("Baía Nova FC"), "sem strict continua");
    }

    #[test]
    fn sair_encerra_e_ajuda_lista_os_comandos() {
        let mut s = session();
        let (out, ok) = run_script(&mut s, "ajuda\nsair\nstatus\n", true);
        assert!(ok);
        assert!(out.contains("comandos:") && out.contains("ao-vivo"));
        assert!(!out.contains("Baía Nova FC"), "nada roda depois de `sair`");
    }

    fn career_dir() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/career")
    }

    #[test]
    fn carreira_versionada_reabre_identica_criterio_de_mvp_4() {
        // `career_s42.sav` foi gravado uma vez (`tests/career/play.txt`) e é
        // commitado; a CI roda este mesmo confronto em Windows, macOS e
        // Linux: o save abre e a carreira continua byte a byte igual
        // (docs/00 §7, critério 4).
        let dir = career_dir();
        let mut s = GameSession::load_from_path(
            &crate::default_example_pack_path(),
            &dir.join("career_s42.sav"),
        )
        .unwrap();
        let script = std::fs::read_to_string(dir.join("verify.txt")).unwrap();
        let expected = std::fs::read_to_string(dir.join("verify.expected.txt"))
            .unwrap()
            .replace("\r\n", "\n");
        let mut out = Vec::new();
        let ok = run(&mut s, script.as_bytes(), &mut out, true, true).unwrap();
        assert!(ok);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            expected,
            "saída divergiu do esperado — se a mudança é intencional, regrave seguindo o \
             cabeçalho de tests/career/play.txt (nunca só pra a CI passar)"
        );
    }

    #[test]
    fn roteiro_de_gravacao_reproduz_exatamente_o_save_versionado() {
        let dir = career_dir();
        let mut s = session_for_career();
        let tmp = std::env::temp_dir().join(format!("managerfc-career-{}.sav", std::process::id()));
        let script = std::fs::read_to_string(dir.join("play.txt"))
            .unwrap()
            .replace("tests/career/career_s42.sav", &tmp.display().to_string());
        let mut out = Vec::new();
        let ok = run(&mut s, script.as_bytes(), &mut out, true, true).unwrap();
        assert!(ok, "{}", String::from_utf8_lossy(&out));
        assert_eq!(
            std::fs::read(&tmp).unwrap(),
            std::fs::read(dir.join("career_s42.sav")).unwrap(),
            "o roteiro play.txt não gera mais o save versionado"
        );
        std::fs::remove_file(&tmp).ok();
    }

    fn session_for_career() -> GameSession {
        let mut s = GameSession::new(&crate::default_example_pack_path(), 42).unwrap();
        s.dispatch(Command::TakeClub {
            club: ClubId::new(3),
            coach: app::CoachProfile {
                name: "Zé Treinador".to_string(),
                nation: domain::NationId::new(0),
                reputation: 30,
            },
        })
        .unwrap();
        s
    }

    #[test]
    fn sessao_sem_tecnico_responde_erro_claro_em_vez_de_panicar() {
        let mut s = GameSession::new(&crate::default_example_pack_path(), 42).unwrap();
        let (out, ok) = run_script(&mut s, "status\nelenco\nescalacao\n", false);
        assert!(!ok);
        assert_eq!(
            out.matches("erro: nenhum clube sob comando").count(),
            3,
            "{out}"
        );
    }
}
