use super::{GameOpts, PlayerId, ROUND_DURATION};
use crate::data::Either;
use crate::{client::Username, data::Line};
use actix::SpawnHandle;
use rand::prelude::IteratorRandom;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::{
    cmp::{max, min},
    time,
};
use time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GamePlayer {
    pub username: Username,
    pub score: u32,
    pub has_solved: bool,
}

impl GamePlayer {
    fn new(username: Username) -> Self {
        GamePlayer {
            username,
            score: 0,
            has_solved: false,
        }
    }
    pub fn on_solve(&mut self, remaining_time: u32) {
        self.score += calculate_score_increase(remaining_time);
        self.has_solved = true;
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SkribblState {
    /// the current round number
    current_round: usize,

    ///the last round number
    last_round: usize,

    turn_end_time: u64,

    word_length: usize,

    revealed_characters: HashMap<usize, char>,

    /// a canvas is a vec of user drawn `Line` to the server.
    pub canvas: Vec<Line>,

    /// players which didn't draw yet in the current round.
    pub remaining_players: Vec<PlayerId>,

    /// states of all the players
    pub players: HashMap<PlayerId, GamePlayer>,

    /// the currently drawing user
    pub drawing_user: PlayerId,
}

impl SkribblState {
    fn new(players: Vec<(PlayerId, Username)>, game_opts: &GameOpts) -> Self {
        let mut state = Self {
            current_round: 0,
            last_round: game_opts.number_of_rounds,
            turn_end_time: 0,
            word_length: 0,
            revealed_characters: HashMap::new(),
            remaining_players: Vec::new(),
            canvas: Vec::new(),
            players: HashMap::new(),
            drawing_user: 0,
        };

        for (id, username) in players {
            state.players.insert(id, GamePlayer::new(username));
        }

        state
    }

    fn next(&mut self, word: &str) {
        self.canvas.clear();
        self.word_length = word.len();
        self.turn_end_time = get_time_now() + ROUND_DURATION;
        self.revealed_characters.clear();

        // reveal all whitespace characters
        for (idx, ch) in word.chars().enumerate() {
            if ch.is_whitespace() {
                self.revealed_characters.insert(idx, ch);
            }
        }

        if self.remaining_players.is_empty() {
            // round over, start new round.
            self.remaining_players = self.players.keys().cloned().collect();
            self.current_round += 1;
        }

        self.drawing_user = self.remaining_players.pop().unwrap();
        self.players
            .values_mut()
            .for_each(|player| player.has_solved = false);
    }

    pub fn is_drawing(&self, player_id: &PlayerId) -> bool {
        self.drawing_user == *player_id
    }

    pub fn has_solved(&self, player_id: &PlayerId) -> bool {
        self.players.get(player_id).map(|x| x.has_solved) == Some(true)
    }

    pub fn remaining_round_time(&self) -> u32 {
        max(0, self.turn_end_time as i64 - get_time_now() as i64) as u32
    }

    /// returns the placeholder chars for the current word, with the revealed characters revealed.
    pub fn hinted_current_word(&self) -> String {
        (0..self.word_length)
            .map(|idx| self.revealed_characters.get(&idx).unwrap_or(&'?'))
            .collect()
    }

    fn can_reveal_char(&self) -> bool {
        self.revealed_characters.len() < self.word_length / 2
    }

    pub fn end_turn(&mut self) {
        let remaining_time = self.remaining_round_time();
        if let Some(drawing_player) = self.players.get_mut(&self.drawing_user) {
            drawing_player.score += 50;
            drawing_player.on_solve(remaining_time);
        }
    }
}

pub struct Skribbl {
    /// the word to guess
    current_word: String,

    /// game state to share to all users.
    pub state: SkribblState,

    pub game_opts: GameOpts,

    pub turn_timer: Option<SpawnHandle>,
}

impl Skribbl {
    pub fn new(players: Vec<(PlayerId, Username)>, mut game_opts: GameOpts) -> Self {
        game_opts.words = match game_opts.words {
            Either::Left(words) => Either::Right(words.into_iter().cycle()),
            Either::Right(words) => Either::Right(words),
        };

        Skribbl {
            current_word: "".to_owned(),
            state: SkribblState::new(players, &game_opts),
            game_opts,
            turn_timer: None,
        }
    }

    /// end current turn
    fn on_turn_end(&mut self) {
        self.state.end_turn();
    }

    pub fn next_turn(&mut self) {
        let words = if let Either::Right(ref mut words) = self.game_opts.words {
            words
        } else {
            return;
        };

        let mut rng = rand::thread_rng();
        let random_word = words.choose(&mut rng).unwrap();
        // let random_word = &(self.game_opts.words)
        //     .right()
        //     .unwrap()
        //     .borrow_mut()
        //     .choose(&mut rng)
        //     .unwrap()
        //     .to_owned();

        let random_word = random_word.to_owned();

        self.state.next(&random_word);
        self.current_word = random_word;
    }

    /// get all of ids of players who cannot guess in current turn.
    pub fn get_non_guessing_players(&self) -> Vec<PlayerId> {
        let drawing_user = self.state.drawing_user;
        self.state
            .players
            .iter()
            .filter_map(|(id, player)| {
                if player.has_solved || *id == drawing_user {
                    Some(*id)
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn current_word(&self) -> &str {
        &self.current_word
    }

    /// reveals a random character, as long as that doesn't reveal half of the word
    pub fn reveal_random_char(&mut self) {
        if self.state.can_reveal_char() {
            let mut rng = rand::thread_rng();

            let (idx, ch) = self
                .current_word
                .chars()
                .enumerate()
                .filter(|(idx, _)| !self.state.revealed_characters.contains_key(&idx))
                .choose(&mut rng)
                .unwrap();

            self.state.revealed_characters.insert(idx, ch);
        }
    }

    pub fn clear_canvas(&mut self) {
        self.state.canvas.clear();
    }

    fn is_drawing(&self, id: &PlayerId) -> bool {
        self.state.drawing_user == *id
    }

    /// whether the given player can guess in the current turn.
    fn can_guess(&self, id: &PlayerId) -> bool {
        !self.is_drawing(id)
            && !self
                .state
                .players
                .get(&id)
                .map(|x| x.has_solved)
                .unwrap_or(false)
    }

    /// whether any player has solved this round.
    pub fn has_any_solved(&self) -> bool {
        self.state
            .players
            .iter()
            .all(|(id, player)| player.has_solved || id == &self.state.drawing_user)
    }

    /// do guess for a player by id, returns the levenshtein_distance of the guess.
    pub fn do_guess(&mut self, id: &PlayerId, guess: &str) -> Option<usize> {
        if self.can_guess(id) {
            let remaining_time = self.state.remaining_round_time();
            let levenshtein_distance = levenshtein_distance(guess, self.current_word());

            if levenshtein_distance == 0 {
                if self.has_any_solved() {
                    self.state.turn_end_time -= remaining_time as u64 / 2;
                }

                self.state
                    .players
                    .get_mut(id)
                    .expect("could not find player by id in game room.")
                    .on_solve(remaining_time);
            }

            return Some(levenshtein_distance);
        }

        None
    }

    pub fn has_turn_ended(&self) -> bool {
        self.state.players.values().all(|player| player.has_solved)
    }

    pub fn has_round_ended(&self) -> bool {
        self.state.remaining_players.is_empty() || self.state.turn_end_time <= get_time_now()
    }

    pub fn is_finished(&self) -> bool {
        self.has_round_ended() && self.state.current_round == self.game_opts.number_of_rounds
    }
    pub fn end_turn(&mut self) {}

    pub fn add_player(&mut self, id: PlayerId, username: Username) {
        if !self.state.players.contains_key(&id) {
            self.state.remaining_players.push(id);
            self.state.players.insert(id, GamePlayer::new(username));
        }
    }
}

fn get_time_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

pub fn calculate_score_increase(remaining_time: u32) -> u32 {
    50 + (((remaining_time as f64 / ROUND_DURATION as f64) * 100f64) as u32 / 2u32)
}

fn levenshtein_distance(a: &str, b: &str) -> usize {
    let w1 = a.chars().collect::<Vec<_>>();
    let w2 = b.chars().collect::<Vec<_>>();

    let a_len = w1.len() + 1;
    let b_len = w2.len() + 1;

    let mut matrix = vec![vec![0]];

    for i in 1..a_len {
        matrix[0].push(i);
    }
    for j in 1..b_len {
        matrix.push(vec![j]);
    }

    for (j, i) in (1..b_len).flat_map(|j| (1..a_len).map(move |i| (j, i))) {
        let x: usize = if w1[i - 1].eq_ignore_ascii_case(&w2[j - 1]) {
            matrix[j - 1][i - 1]
        } else {
            1 + min(
                min(matrix[j][i - 1], matrix[j - 1][i]),
                matrix[j - 1][i - 1],
            )
        };
        matrix[j].push(x);
    }

    matrix[b_len - 1][a_len - 1]
}
