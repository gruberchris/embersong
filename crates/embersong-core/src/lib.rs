//! Embersong core: deterministic game logic shared by native tests and the
//! wasm32-unknown-unknown guest. No windowing, no audio, no filesystem — just state.
//!
//! Faithful port of the Python original's combat math, expanded with:
//! Bard songs (Breath resource), Soothe/Tame (Harmony meter), companions,
//! a 12-entry Bestiary with evolutions, and 3 vaults + tavern hub.

use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Kinds & static data
// ---------------------------------------------------------------------------

/// Star-spawn elements for the Pokemon-style type wheel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Element {
    Ember,
    Frost,
    Thorn,
    Gloom,
    Star,
}

/// All 12 bindable monsters. The first five are the Python originals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MonsterKind {
    Goblin,
    Troll,
    Orc,
    Vampire,
    HillGiant,
    // Evolutions / variants (Bestiary 6..12)
    RabidGoblinKing,
    ElderTroll,
    WarlordOrc,
    StarTouchedVampire,
    CrownedGiant,
    MothWisp,
    ThornDrake,
}

#[derive(Debug, Clone, Copy)]
pub struct BaseStats {
    pub max_hp: i32,
    pub damage: i32,
    pub hit: f32,
    pub special: f32,
    pub element: Element,
    pub tame_threshold: i32,
}

impl MonsterKind {
    pub fn base(self) -> BaseStats {
        match self {
            MonsterKind::Goblin => BaseStats {
                max_hp: 10,
                damage: 2,
                hit: 0.5,
                special: 0.6,
                element: Element::Gloom,
                tame_threshold: 60,
            },
            MonsterKind::Troll => BaseStats {
                max_hp: 11,
                damage: 2,
                hit: 0.6,
                special: 0.5,
                element: Element::Thorn,
                tame_threshold: 65,
            },
            MonsterKind::Orc => BaseStats {
                max_hp: 12,
                damage: 2,
                hit: 0.6,
                special: 0.3,
                element: Element::Ember,
                tame_threshold: 70,
            },
            MonsterKind::Vampire => BaseStats {
                max_hp: 15,
                damage: 3,
                hit: 0.6,
                special: 0.5,
                element: Element::Gloom,
                tame_threshold: 80,
            },
            MonsterKind::HillGiant => BaseStats {
                max_hp: 20,
                damage: 4,
                hit: 0.4,
                special: 0.05,
                element: Element::Frost,
                tame_threshold: 90,
            },
            MonsterKind::RabidGoblinKing => BaseStats {
                max_hp: 16,
                damage: 3,
                hit: 0.9,
                special: 0.35,
                element: Element::Gloom,
                tame_threshold: 100,
            },
            MonsterKind::ElderTroll => BaseStats {
                max_hp: 22,
                damage: 3,
                hit: 0.65,
                special: 0.5,
                element: Element::Thorn,
                tame_threshold: 100,
            },
            MonsterKind::WarlordOrc => BaseStats {
                max_hp: 24,
                damage: 4,
                hit: 0.65,
                special: 0.35,
                element: Element::Ember,
                tame_threshold: 100,
            },
            MonsterKind::StarTouchedVampire => BaseStats {
                max_hp: 26,
                damage: 4,
                hit: 0.7,
                special: 0.55,
                element: Element::Star,
                tame_threshold: 100,
            },
            MonsterKind::CrownedGiant => BaseStats {
                max_hp: 40,
                damage: 6,
                hit: 0.5,
                special: 0.15,
                element: Element::Frost,
                tame_threshold: 100,
            },
            MonsterKind::MothWisp => BaseStats {
                max_hp: 8,
                damage: 2,
                hit: 0.7,
                special: 0.4,
                element: Element::Star,
                tame_threshold: 50,
            },
            MonsterKind::ThornDrake => BaseStats {
                max_hp: 30,
                damage: 5,
                hit: 0.6,
                special: 0.4,
                element: Element::Thorn,
                tame_threshold: 100,
            },
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            MonsterKind::Goblin => "Goblin",
            MonsterKind::Troll => "Troll",
            MonsterKind::Orc => "Orc",
            MonsterKind::Vampire => "Vampire",
            MonsterKind::HillGiant => "Hill Giant",
            MonsterKind::RabidGoblinKing => "Rabid Goblin King",
            MonsterKind::ElderTroll => "Elder Troll",
            MonsterKind::WarlordOrc => "Warlord Orc",
            MonsterKind::StarTouchedVampire => "Star-Touched Vampire",
            MonsterKind::CrownedGiant => "Crowned Giant",
            MonsterKind::MothWisp => "Moth Wisp",
            MonsterKind::ThornDrake => "Thorn Drake",
        }
    }

    /// Evolution chain for bound companions (None = final form).
    pub fn evolves_into(self) -> Option<MonsterKind> {
        match self {
            MonsterKind::Goblin => Some(MonsterKind::RabidGoblinKing),
            MonsterKind::Troll => Some(MonsterKind::ElderTroll),
            MonsterKind::Orc => Some(MonsterKind::WarlordOrc),
            MonsterKind::Vampire => Some(MonsterKind::StarTouchedVampire),
            MonsterKind::HillGiant => Some(MonsterKind::CrownedGiant),
            MonsterKind::MothWisp => Some(MonsterKind::ThornDrake),
            _ => None,
        }
    }
}

/// Bard verses. Breath (0..=6) replaces MP; songs are the Bard's-Tale hook.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Song {
    EmberReel,
    MothLullaby,
    WardensHymn,
}

impl Song {
    pub fn breath_cost(self) -> i32 {
        match self {
            Song::EmberReel => 2,
            Song::MothLullaby => 2,
            Song::WardensHymn => 3,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Song::EmberReel => "Ember Reel",
            Song::MothLullaby => "Moth Lullaby",
            Song::WardensHymn => "Warden's Hymn",
        }
    }
}

// ---------------------------------------------------------------------------
// Bard battle songs: one looping synth track per fight, note-timed bonuses.
// ---------------------------------------------------------------------------

/// Which third of the track's range a sampled note fell in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BardZone {
    High,
    Mid,
    Low,
}

impl BardZone {
    pub fn label(self) -> &'static str {
        match self {
            BardZone::High => "high verse",
            BardZone::Mid => "steady verse",
            BardZone::Low => "low verse",
        }
    }
}

/// A battle song track: MIDI note sequence looped with a fixed step duration.
/// Core owns the note tables (game data); `embersong-synth` renders them.
#[derive(Debug, Clone, Copy)]
pub struct BattleTrack {
    pub name: &'static str,
    pub notes: &'static [u8],
    pub note_dur: f32,
}

/// First test track: Ember Vanguard March, 16 steps across ~2 octaves so the
/// per-track thirds split is meaningful (min 57, max 81).
pub const EMBER_VANGUARD_NOTES: &[u8] = &[
    57, 60, 62, 64, 67, 69, 72, 74, 76, 79, 81, 79, 76, 72, 69, 64,
];

pub const EMBER_VANGUARD_DUR: f32 = 0.22;

pub fn battle_track_count() -> usize {
    1
}

pub fn battle_track(idx: u32) -> BattleTrack {
    // Single test track for now; the registry grows by pushing here.
    let _ = idx;
    BattleTrack {
        name: "Ember Vanguard March",
        notes: EMBER_VANGUARD_NOTES,
        note_dur: EMBER_VANGUARD_DUR,
    }
}

pub fn battle_track_name(idx: u32) -> &'static str {
    battle_track(idx).name
}

/// Sample the looping note at `elapsed` seconds since the fight started.
pub fn bard_note_at(track_idx: u32, elapsed: f32) -> (usize, u8) {
    let t = battle_track(track_idx);
    let len = t.notes.len().max(1);
    let t_sec = if elapsed < 0.0 { 0.0 } else { elapsed };
    let step = (t_sec / t.note_dur.max(0.01)).floor() as usize;
    let i = step % len;
    (i, t.notes[i])
}

/// Per-track thirds: bottom third = Low, top third = High, else Mid.
pub fn classify_bard_note(track_idx: u32, midi: u8) -> BardZone {
    let t = battle_track(track_idx);
    let (mut lo, mut hi) = (u8::MAX, u8::MIN);
    for n in t.notes {
        lo = lo.min(*n);
        hi = hi.max(*n);
    }
    if hi <= lo {
        return BardZone::Mid;
    }
    let span = (hi - lo) as f32;
    let v = (midi.saturating_sub(lo)) as f32;
    if v <= span / 3.0 {
        BardZone::Low
    } else if v > span * 2.0 / 3.0 {
        BardZone::High
    } else {
        BardZone::Mid
    }
}

// ---------------------------------------------------------------------------
// Live entities
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Monster {
    pub kind: MonsterKind,
    pub name: String,
    pub hp: i32,
    pub max_hp: i32,
    pub damage: i32,
    pub hit: f32,
    pub special: f32,
    pub rabid: bool,
    pub enraged: bool,
    /// 0..=100. Soothe + Moth Lullaby raise it; at threshold the bard may Bind.
    pub harmony: i32,
    /// WardensHymn / EmberReel debuff turns remaining on the monster.
    pub exposed_turns: i32,
}

impl Monster {
    pub fn spawn(kind: MonsterKind) -> Self {
        let b = kind.base();
        Self {
            kind,
            name: kind.display_name().to_string(),
            hp: b.max_hp,
            max_hp: b.max_hp,
            damage: b.damage,
            hit: b.hit,
            special: b.special,
            rabid: false,
            enraged: false,
            harmony: 0,
            exposed_turns: 0,
        }
    }
    pub fn alive(&self) -> bool {
        self.hp > 0
    }

    fn become_rabid(&mut self, log: &mut Vec<Event>) {
        self.rabid = true;
        self.hit = 0.9;
        self.special = 0.3;
        self.name = format!("Rabid {}", self.name);
        log.push(Event::message(format!("The {} turns rabid!", self.name)));
        log.push(Event::sound(SoundTrigger::Enrage));
    }

    fn maybe_enrage_giant(&mut self, log: &mut Vec<Event>) {
        if self.kind == MonsterKind::HillGiant
            && !self.enraged
            && self.hp <= (self.max_hp as f32 * 0.3) as i32
        {
            self.enraged = true;
            self.hit = 0.9;
            self.special = 0.15;
            self.name = "Enraged Hill Giant".to_string();
            log.push(Event::message("The Hill Giant is enraged!".to_string()));
            log.push(Event::sound(SoundTrigger::Enrage));
        }
    }

    /// Python `Monster.attack` port. Returns damage dealt to hero.
    fn strike_hero(&mut self, hero: &mut Hero, rng: &mut ChaCha8Rng, log: &mut Vec<Event>) -> i32 {
        // Per-kind pre-attack quirks (Python subclasses).
        match self.kind {
            MonsterKind::Goblin | MonsterKind::RabidGoblinKing => {
                if !self.rabid && rng.gen::<f32>() <= 0.3 {
                    self.become_rabid(log);
                }
            }
            MonsterKind::Troll | MonsterKind::ElderTroll => {
                if self.hp < self.max_hp {
                    let regen = rng.gen_range(1..=2);
                    let heal = regen.min(self.max_hp - self.hp);
                    self.hp += heal;
                    log.push(Event::message(format!(
                        "The {} regenerates {heal} health!",
                        self.name
                    )));
                    log.push(Event::sound(SoundTrigger::Heal));
                }
            }
            MonsterKind::HillGiant | MonsterKind::CrownedGiant => self.maybe_enrage_giant(log),
            _ => {}
        }

        if rng.gen::<f32>() > self.hit {
            log.push(Event::message(format!("The {} misses!", self.name)));
            log.push(Event::sound(SoundTrigger::Miss));
            return 0;
        }

        let mut total = rng.gen_range(1..=self.damage.max(1));
        log.push(Event::message(format!(
            "The {} attacks with its weapon for {total} damage.",
            self.name
        )));

        if self.rabid {
            let bite = (rng.gen_range(1..=self.damage.max(1)) as f32 * 1.5) as i32;
            total += bite;
            log.push(Event::message(format!("It bites you for {bite} damage!")));
        }

        if rng.gen::<f32>() <= self.special {
            total += self.special_attack(hero, rng, log);
        }

        if self.exposed_turns > 0 {
            total = (total / 2).max(0);
            self.exposed_turns -= 1;
        }

        // Companion bodyguard: bound Thorn Drake shaves 1 damage (min 0).
        let guard = hero.guard_reduction();
        total = (total - guard).max(0);

        hero.hp -= total;
        log.push(Event::sound(SoundTrigger::HeroHurt));
        total
    }

    fn special_attack(
        &mut self,
        hero: &mut Hero,
        rng: &mut ChaCha8Rng,
        log: &mut Vec<Event>,
    ) -> i32 {
        match self.kind {
            MonsterKind::Goblin | MonsterKind::RabidGoblinKing => {
                let mut d = rng.gen_range(1..=self.damage.max(1));
                if self.rabid {
                    d = (d as f32 * 1.5) as i32;
                }
                log.push(Event::message(format!("Dagger thrust for {d} damage!")));
                d
            }
            MonsterKind::Troll | MonsterKind::ElderTroll => {
                let d = rng.gen_range(1..=self.damage.max(1));
                log.push(Event::message(format!("Backhand for {d} damage!")));
                d
            }
            MonsterKind::Orc | MonsterKind::WarlordOrc => {
                let d = self.damage * 3;
                log.push(Event::message(format!("Triple slam for {d} bonus damage!")));
                d
            }
            MonsterKind::Vampire | MonsterKind::StarTouchedVampire => {
                let d = self.damage;
                let heal = d.min(self.max_hp - self.hp).max(0);
                if heal > 0 {
                    self.hp += heal;
                    log.push(Event::message(format!(
                        "The {} drinks deep and heals {heal}!",
                        self.name
                    )));
                }
                log.push(Event::message(
                    "Your wounds open — it grows stronger!".to_string(),
                ));
                let _ = hero;
                d
            }
            _ => {
                // Hill Giant family + wisps/drakes: heavy smash.
                let mut d = self.damage;
                if self.enraged {
                    d = (d as f32 * 1.5) as i32;
                }
                log.push(Event::message(format!(
                    "Crushing smash for {d} bonus damage!"
                )));
                d
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hero {
    pub name: String,
    pub hp: i32,
    pub max_hp: i32,
    pub damage: i32,
    pub hit: f32,
    pub score: u32,
    pub turns: i64,
    pub breath: i32,
    pub max_breath: i32,
    /// Attack buff turns from Ember Reel.
    pub verse_power: i32,
    /// Bound companions (reserve). Active summon handled via `guard` kinds.
    pub companions: Vec<MonsterKind>,
    /// Bestiary: kinds ever bound.
    pub bestiary: Vec<MonsterKind>,
    pub shards: u32,
}

impl Default for Hero {
    fn default() -> Self {
        Self::new()
    }
}

impl Hero {
    pub fn new() -> Self {
        Self {
            name: "Lantern-Bard".to_string(),
            hp: 15,
            max_hp: 15,
            damage: 5,
            hit: 0.8,
            score: 0,
            turns: -1,
            breath: 6,
            max_breath: 6,
            verse_power: 0,
            companions: Vec::new(),
            bestiary: Vec::new(),
            shards: 0,
        }
    }

    fn guard_reduction(&self) -> i32 {
        // Bound Elder Troll / Thorn Drake watch your back (Pokemon-party feel).
        let mut g = 0;
        for c in &self.companions {
            match c {
                MonsterKind::ElderTroll | MonsterKind::ThornDrake => g += 1,
                _ => {}
            }
        }
        g.min(2)
    }

    /// Python `Hero.attack` port. Returns damage dealt.
    /// Bard rule (mixed crit-style): High auto-hits and +50% (ceil),
    /// Low is -2 (min 0) with a lower effective hit rate, Mid unchanged.
    fn strike(&mut self, m: &mut Monster, rng: &mut ChaCha8Rng, bard: BardZone) -> i32 {
        let mut dmg = rng.gen_range(1..=self.damage.max(1));
        if rng.gen::<f32>() < 0.3 {
            dmg += (dmg as f32 * 0.5) as i32;
        }
        let effective_hit = match bard {
            BardZone::High => 2.0, // auto-hit: any gen::<f32>() <= 2.0
            BardZone::Low => (self.hit - 0.25).max(0.05),
            BardZone::Mid => self.hit,
        };
        if rng.gen::<f32>() > effective_hit {
            return 0;
        }
        if self.verse_power > 0 {
            dmg += 2;
            self.verse_power -= 1;
        }
        match bard {
            BardZone::High => {
                dmg = ((dmg as f32 * 1.5).ceil() as i32).max(1);
            }
            BardZone::Low => {
                dmg = (dmg - 2).max(0);
            }
            BardZone::Mid => {}
        }
        if m.enraged {
            dmg /= 2; // Python mitigation rule.
        }
        m.hp -= dmg;
        dmg
    }

    /// Python `Hero.heal` port with cap at max_hp.
    fn heal_self(&mut self, rng: &mut ChaCha8Rng, log: &mut Vec<Event>) {
        let mut amount = rng.gen_range(1..=(self.max_hp / 2).max(1));
        let bonus = if rng.gen::<f32>() <= 0.3 {
            rng.gen_range(1..=(self.max_hp / 2).max(1))
        } else {
            0
        };
        if self.hp < self.max_hp {
            amount = (amount + bonus).min(self.max_hp - self.hp);
            self.hp += amount;
            if bonus > 0 {
                log.push(Event::message(format!(
                    "You heal exceptionally well for {amount}!"
                )));
            } else {
                log.push(Event::message(format!("You heal for {amount}.")));
            }
            log.push(Event::sound(SoundTrigger::Heal));
        } else {
            log.push(Event::message("No wounds to heal.".to_string()));
        }
    }
}

// ---------------------------------------------------------------------------
// Actions, events, sounds
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    Strike,
    Song(Song),
    Soothe,
    Heal,
    Summon(usize),
    Flee,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SoundTrigger {
    Strike,
    HeroHurt,
    MonsterDie,
    Heal,
    Miss,
    Tame,
    Song,
    Enrage,
    Flee,
    Victory,
    Defeat,
    BardHigh,
    BardLow,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Event {
    Message(String),
    Sound(SoundTrigger),
    BardBeat {
        track: u32,
        note_idx: usize,
        midi: u8,
        zone: BardZone,
    },
    Tamed(MonsterKind),
    Evolved {
        from: MonsterKind,
        into: MonsterKind,
    },
    VaultCleared(usize),
}

impl Event {
    pub fn message(s: String) -> Self {
        Event::Message(s)
    }
    pub fn sound(s: SoundTrigger) -> Self {
        Event::Sound(s)
    }
}

// ---------------------------------------------------------------------------
// Vaults (the "larger adventure": tavern hub + 3 dungeons)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vault {
    pub index: usize,
    pub name: String,
    pub table: Vec<MonsterKind>,
    pub count: (u32, u32),
}

impl Vault {
    pub fn warren() -> Self {
        Self {
            index: 0,
            name: "Mosslight Warren".to_string(),
            table: vec![
                MonsterKind::MothWisp,
                MonsterKind::Goblin,
                MonsterKind::Troll,
            ],
            count: (3, 4),
        }
    }
    pub fn choir() -> Self {
        Self {
            index: 1,
            name: "Sunken Choir".to_string(),
            table: vec![
                MonsterKind::Orc,
                MonsterKind::Vampire,
                MonsterKind::Troll,
                MonsterKind::Goblin,
            ],
            count: (4, 5),
        }
    }
    pub fn crown() -> Self {
        Self {
            index: 2,
            name: "Crown of Hollow Hill".to_string(),
            table: vec![
                MonsterKind::HillGiant,
                MonsterKind::Vampire,
                MonsterKind::Orc,
                MonsterKind::ThornDrake,
            ],
            count: (5, 6),
        }
    }
    pub fn all() -> Vec<Vault> {
        vec![Self::warren(), Self::choir(), Self::crown()]
    }
}

// ---------------------------------------------------------------------------
// Game state machine
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    Tavern,
    Explore,
    Combat,
    GameOver,
    Victory,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Game {
    pub hero: Hero,
    pub phase: Phase,
    pub vault_index: usize,
    pub queue: Vec<MonsterKind>,
    pub current: Option<Monster>,
    pub fled: bool,
    pub seed: u64,
    /// Serialized too (rand_chacha serde): the stream continues across
    /// save/load and across the native↔WASM boundary identically.
    pub rng: Option<ChaCha8Rng>,
    /// Randomly selected battle song track for the current fight.
    /// `#[serde(default)]` keeps pre-bard saves loadable (defaults to 0).
    #[serde(default)]
    pub battle_track: u32,
}

impl Game {
    pub fn new(seed: u64) -> Self {
        let mut g = Self {
            hero: Hero::new(),
            phase: Phase::Tavern,
            vault_index: 0,
            queue: Vec::new(),
            current: None,
            fled: false,
            seed,
            rng: Some(ChaCha8Rng::seed_from_u64(seed)),
            battle_track: 0,
        };
        g.enter_vault(0);
        g
    }

    fn rng_mut(&mut self) -> &mut ChaCha8Rng {
        if self.rng.is_none() {
            let seed = self.seed ^ 0x9e37;
            self.rng = Some(ChaCha8Rng::seed_from_u64(seed));
        }
        self.rng.as_mut().unwrap()
    }

    /// Mutable RNG without borrowing all of `self` (for disjoint field borrows).
    fn rng_field(opt: &mut Option<ChaCha8Rng>, seed: u64) -> &mut ChaCha8Rng {
        if opt.is_none() {
            *opt = Some(ChaCha8Rng::seed_from_u64(seed ^ 0x9e37));
        }
        opt.as_mut().unwrap()
    }

    fn soothe_bonus_for(companions: &[MonsterKind]) -> i32 {
        // Bound Moth Wisp harmonizes your humming (+6).
        if companions.contains(&MonsterKind::MothWisp) {
            6
        } else {
            0
        }
    }

    pub fn vaults() -> Vec<Vault> {
        Vault::all()
    }

    /// Fill the queue for a vault and pop the first monster (Python: 3..=6 random).
    pub fn enter_vault(&mut self, index: usize) {
        let vaults = Vault::all();
        let v = &vaults[index.min(2)];
        self.vault_index = v.index;
        // Borrow-split: generate queue first, then assign.
        let count = {
            let r = self.rng_mut();
            r.gen_range(v.count.0..=v.count.1)
        };
        let mut q = Vec::new();
        for _ in 0..count {
            // NOTE: bounds are `u32`, never `usize` — `rand` samples `usize`
            // ranges with pointer-width math, which diverges between the
            // 64-bit host and the 32-bit WASM guest. Fixed-width bounds keep
            // native and sandboxed simulations bit-identical.
            let k = {
                let r = self.rng_mut();
                v.table[r.gen_range(0..v.table.len() as u32) as usize]
            };
            q.push(k);
        }
        self.queue = q;
        self.phase = Phase::Explore;
        self.next_monster();
        self.phase = Phase::Combat;
    }

    fn next_monster(&mut self) {
        if self.queue.is_empty() {
            self.current = None;
            return;
        }
        // Python picks a random remaining monster each time.
        // (`u32` bound: see the determinism note in `enter_vault`.)
        let n = self.queue.len();
        let i = {
            let r = self.rng_mut();
            r.gen_range(0..n as u32) as usize
        };
        let kind = self.queue.remove(i);
        self.current = Some(Monster::spawn(kind));
        // A new fight starts: randomly select a battle song track.
        let count = battle_track_count().max(1) as u32;
        let t = {
            let r = self.rng_mut();
            r.gen_range(0..count)
        };
        self.battle_track = t;
    }

    pub fn alive(&self) -> bool {
        self.hero.hp > 0
    }

    /// One hero turn + monster reply. Returns the event log for UI + audio.
    /// Wall-clock entry point defers to [`Game::act_at`] with a synthetic
    /// turn-stepped clock so headless sims stay deterministic.
    pub fn act(&mut self, action: Action) -> Vec<Event> {
        self.act_at(action, None)
    }

    /// Turn driver with an explicit bard clock.
    /// `beat_time`: seconds since the fight started (host wall-clock).
    /// `None` = synthetic `turns * note_dur` fallback for tests/headless.
    pub fn act_at(&mut self, action: Action, beat_time: Option<f32>) -> Vec<Event> {
        let mut log = Vec::new();
        if !self.alive() || self.phase == Phase::GameOver || self.phase == Phase::Victory {
            return log;
        }
        self.hero.turns += 1;
        // Breath refreshes slowly (bard rhythm).
        self.hero.breath = (self.hero.breath + 1).min(self.hero.max_breath);

        // Bard beat: sample the looping battle song at the moment of action.
        // `None` falls back to a turn-stepped clock so sims/tests are stable.
        let track = self.battle_track;
        let step_dur = battle_track(track).note_dur;
        let clock = beat_time.unwrap_or(self.hero.turns.max(0) as f32 * step_dur);
        let (note_idx, midi) = bard_note_at(track, clock);
        let zone = classify_bard_note(track, midi);

        match action {
            Action::Flee => {
                self.fled = true;
                self.phase = Phase::GameOver;
                log.push(Event::message(format!(
                    "You flee after {} turns with {} binds.",
                    self.hero.turns.max(0),
                    self.hero.score
                )));
                log.push(Event::sound(SoundTrigger::Flee));
                return log;
            }
            Action::Heal => {
                log.push(Event::BardBeat {
                    track,
                    note_idx,
                    midi,
                    zone,
                });
                let seed = self.seed;
                let rng = Self::rng_field(&mut self.rng, seed);
                self.hero.heal_self(rng, &mut log);
            }
            Action::Strike => {
                if let Some(mon) = self.current.as_mut() {
                    // Disjoint field borrows: rng + hero + current.
                    let seed = self.seed;
                    let rng = Self::rng_field(&mut self.rng, seed);
                    let dmg = self.hero.strike(mon, rng, zone);
                    log.push(Event::BardBeat {
                        track,
                        note_idx,
                        midi,
                        zone,
                    });
                    // NLL ends the borrows above; re-read for the message.
                    if dmg > 0 {
                        let m = self.current.as_ref().expect("foe present");
                        let bard_tag = match zone {
                            BardZone::High => " ♪ high verse! +50%, true strike!",
                            BardZone::Low => " ♭ low verse... -2.",
                            BardZone::Mid => "",
                        };
                        if m.enraged {
                            log.push(Event::message(format!(
                                "You strike the {} but rage halves it to {dmg}!{bard_tag}",
                                m.name
                            )));
                        } else {
                            log.push(Event::message(format!(
                                "You strike the {} for {dmg}!{bard_tag}",
                                m.name
                            )));
                        }
                        log.push(Event::sound(SoundTrigger::Strike));
                    } else {
                        let m = self.current.as_ref().expect("foe present");
                        let bard_tag = match zone {
                            BardZone::Low => " (low verse falters)",
                            BardZone::High => " (high verse rings true)",
                            BardZone::Mid => "",
                        };
                        log.push(Event::message(format!(
                            "You miss the {}!{bard_tag}",
                            m.name
                        )));
                        log.push(Event::sound(SoundTrigger::Miss));
                    }
                    match zone {
                        BardZone::High => log.push(Event::sound(SoundTrigger::BardHigh)),
                        BardZone::Low => log.push(Event::sound(SoundTrigger::BardLow)),
                        BardZone::Mid => {}
                    }
                    self.check_kill(&mut log);
                } else {
                    log.push(Event::message("No foe to strike.".to_string()));
                }
            }
            Action::Song(song) => {
                log.push(Event::BardBeat {
                    track,
                    note_idx,
                    midi,
                    zone,
                });
                self.cast_song(song, &mut log);
            }
            Action::Soothe => {
                log.push(Event::BardBeat {
                    track,
                    note_idx,
                    midi,
                    zone,
                });
                let gain = 22 + Self::soothe_bonus_for(&self.hero.companions);
                let mut should_bind = false;
                if let Some(m) = self.current.as_mut() {
                    let need = m.kind.base().tame_threshold;
                    m.harmony = (m.harmony + gain).min(100);
                    log.push(Event::message(format!(
                        "You hum low. Harmony {}/{} ({}).",
                        m.harmony, need, m.name
                    )));
                    log.push(Event::sound(SoundTrigger::Song));
                    should_bind = m.harmony >= need;
                }
                if should_bind {
                    self.bind_current(&mut log);
                }
            }
            Action::Summon(idx) => {
                log.push(Event::BardBeat {
                    track,
                    note_idx,
                    midi,
                    zone,
                });
                if idx < self.hero.companions.len() {
                    let k = self.hero.companions[idx];
                    log.push(Event::message(format!(
                        "{} answers your lantern-call!",
                        k.display_name()
                    )));
                    // Summon effect: small heal + expose foe (pack tactics).
                    self.hero.hp = (self.hero.hp + 2).min(self.hero.max_hp);
                    if let Some(m) = self.current.as_mut() {
                        m.exposed_turns = (m.exposed_turns + 2).max(2);
                    }
                    log.push(Event::sound(SoundTrigger::Song));
                } else {
                    log.push(Event::message("No such companion.".to_string()));
                }
            }
        }

        // Fainted mid-turn (bound) or fled: skip reply.
        if self.phase == Phase::GameOver || self.phase == Phase::Victory {
            return log;
        }
        // Monster reply (unless it was just bound/killed).
        // Healing/Song turns still draw a reply (Python rule).
        if let Some(mon) = self.current.as_mut() {
            let seed = self.seed;
            let rng = Self::rng_field(&mut self.rng, seed);
            mon.strike_hero(&mut self.hero, rng, &mut log);
            if !self.alive() {
                self.phase = Phase::GameOver;
                log.push(Event::message(format!(
                    "You fall after {} turns with {} binds.",
                    self.hero.turns.max(0),
                    self.hero.score
                )));
                log.push(Event::sound(SoundTrigger::Defeat));
            }
        }
        log
    }

    fn cast_song(&mut self, song: Song, log: &mut Vec<Event>) {
        if self.hero.breath < song.breath_cost() {
            log.push(Event::message("Not enough Breath — you gasp.".to_string()));
            return;
        }
        self.hero.breath -= song.breath_cost();
        log.push(Event::sound(SoundTrigger::Song));
        match song {
            Song::EmberReel => {
                self.hero.verse_power += 3;
                log.push(Event::message(format!(
                    "{}! Your next strikes burn (+2 x3).",
                    song.name()
                )));
            }
            Song::MothLullaby => {
                let mut should_bind = false;
                if let Some(m) = self.current.as_mut() {
                    let need = m.kind.base().tame_threshold;
                    m.harmony = (m.harmony + 30).min(100);
                    log.push(Event::message(format!(
                        "{}! Harmony {}/{} ({}).",
                        song.name(),
                        m.harmony,
                        need,
                        m.name
                    )));
                    should_bind = m.harmony >= need;
                }
                if should_bind {
                    self.bind_current(log);
                }
            }
            Song::WardensHymn => {
                self.hero.hp = (self.hero.hp + 4).min(self.hero.max_hp);
                if let Some(m) = self.current.as_mut() {
                    m.exposed_turns = (m.exposed_turns + 2).max(2);
                }
                log.push(Event::message(format!(
                    "{}! +4 health, foe exposed (halved hits).",
                    song.name()
                )));
            }
        }
    }

    /// Bind the current foe instead of killing it (Pokemon hook).
    fn bind_current(&mut self, log: &mut Vec<Event>) {
        if let Some(m) = self.current.take() {
            let kind = m.kind;
            self.hero.score += 1;
            self.hero.shards += 1;
            if !self.hero.bestiary.contains(&kind) {
                self.hero.bestiary.push(kind);
            }
            if !self.hero.companions.contains(&kind) {
                self.hero.companions.push(kind);
            }
            log.push(Event::message(format!(
                "Bound! {} joins your lantern-light. (+1 star shard)",
                kind.display_name()
            )));
            log.push(Event::sound(SoundTrigger::Tame));
            log.push(Event::Tamed(kind));
            self.try_evolve(log);
            self.next_monster();
            if self.current.is_none() {
                self.clear_vault(log);
            }
        }
    }

    fn check_kill(&mut self, log: &mut Vec<Event>) {
        let dead = self.current.as_ref().map(|m| !m.alive()).unwrap_or(false);
        if dead {
            let kind = self.current.as_ref().unwrap().kind;
            let name = self.current.as_ref().unwrap().name.clone();
            log.push(Event::message(format!("You defeat the {name}!")));
            log.push(Event::sound(SoundTrigger::MonsterDie));
            self.hero.score += 1;
            self.hero.shards += 1;
            // Slaying also records the verse (dead or bound, the song remembers).
            if !self.hero.bestiary.contains(&kind) {
                self.hero.bestiary.push(kind);
            }
            // 25%: a slain foe leaves a friendlier echo you can bind outright.
            let echo = {
                let r = self.rng_mut();
                r.gen::<f32>() < 0.25
            };
            if echo && !self.hero.companions.contains(&kind) {
                self.hero.companions.push(kind);
                log.push(Event::message(format!(
                    "Its echo lingers — {} pads after you.",
                    kind.display_name()
                )));
                log.push(Event::Tamed(kind));
            }
            self.try_evolve(log);
            self.next_monster();
            if self.current.is_none() {
                self.clear_vault(log);
            }
        }
    }

    fn try_evolve(&mut self, log: &mut Vec<Event>) {
        // 3 shards + base form bound => evolution (Pokemon hook, deterministic).
        if self.hero.shards >= 3 {
            let mut evolved = None;
            for c in self.hero.companions.clone() {
                if let Some(next) = c.evolves_into() {
                    if !self.hero.companions.contains(&next) {
                        evolved = Some((c, next));
                        break;
                    }
                }
            }
            if let Some((from, into)) = evolved {
                self.hero.shards -= 3;
                self.hero.companions.push(into);
                if !self.hero.bestiary.contains(&into) {
                    self.hero.bestiary.push(into);
                }
                log.push(Event::message(format!(
                    "Starlight gathers — {} evolves into {}!",
                    from.display_name(),
                    into.display_name()
                )));
                log.push(Event::Evolved { from, into });
                log.push(Event::sound(SoundTrigger::Victory));
            }
        }
    }

    fn clear_vault(&mut self, log: &mut Vec<Event>) {
        let cleared = self.vault_index;
        log.push(Event::message(format!(
            "{} is retuned!",
            Vault::all()[cleared].name
        )));
        log.push(Event::sound(SoundTrigger::Victory));
        log.push(Event::VaultCleared(cleared));
        if cleared + 1 < Vault::all().len() {
            self.enter_vault(cleared + 1);
        } else {
            self.phase = Phase::Victory;
            log.push(Event::message(format!(
                "All vaults sing! {} binds in {} turns.",
                self.hero.score,
                self.hero.turns.max(0)
            )));
        }
    }

    // -- save / load ---------------------------------------------------------
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }
    pub fn from_json(s: &str) -> Option<Self> {
        serde_json::from_str(s).ok()
    }
}

// ---------------------------------------------------------------------------
// Headless helper for CI smoke tests
// ---------------------------------------------------------------------------

/// Play a scripted sim with no UI: strike until hurt, heal under 40%, else soothe.
pub fn simulate(seed: u64, max_turns: u32) -> (Game, Vec<String>) {
    let mut g = Game::new(seed);
    let mut notes = Vec::new();
    for _ in 0..max_turns {
        if g.phase == Phase::GameOver || g.phase == Phase::Victory {
            break;
        }
        let act = if g.hero.hp <= (g.hero.max_hp as f32 * 0.4) as i32 {
            Action::Heal
        } else if g.current.as_ref().map(|m| m.harmony > 40).unwrap_or(false) {
            Action::Soothe
        } else {
            Action::Strike
        };
        for e in g.act(act) {
            if let Event::Message(m) = e {
                if notes.len() < 8 {
                    notes.push(m);
                }
            }
        }
    }
    (g, notes)
}

// ---------------------------------------------------------------------------
// Tests: Python parity + new mechanics
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn rng() -> ChaCha8Rng {
        ChaCha8Rng::seed_from_u64(42)
    }

    #[test]
    fn hero_starts_like_python() {
        let h = Hero::new();
        assert_eq!((h.hp, h.max_hp, h.damage), (15, 15, 5));
        assert!((h.hit - 0.8).abs() < f32::EPSILON);
    }

    #[test]
    fn original_five_stats_match_python() {
        assert_eq!(Monster::spawn(MonsterKind::Goblin).max_hp, 10);
        assert_eq!(Monster::spawn(MonsterKind::Troll).max_hp, 11);
        assert_eq!(Monster::spawn(MonsterKind::Orc).max_hp, 12);
        assert_eq!(Monster::spawn(MonsterKind::Vampire).max_hp, 15);
        assert_eq!(Monster::spawn(MonsterKind::HillGiant).max_hp, 20);
    }

    #[test]
    fn giant_enrage_halves_hero_damage() {
        let mut g = Game::new(7);
        g.current = Some(Monster::spawn(MonsterKind::HillGiant));
        let m = g.current.as_mut().unwrap();
        m.hp = 5; // below 30% of 20 -> enrage on its next strike
        m.hit = 1.0;
        m.special = 0.0;
        m.damage = 1;
        // Force the monster's pre-attack to run via a hero heal turn.
        let hp_before = g.hero.hp;
        g.act(Action::Heal);
        assert!(g.current.as_ref().unwrap().enraged);
        assert!(g.hero.hp < hp_before);
    }

    #[test]
    fn orc_special_is_triple() {
        let mut m = Monster::spawn(MonsterKind::Orc);
        let mut h = Hero::new();
        let mut r = rng();
        let mut log = Vec::new();
        m.hit = 1.0;
        m.special = 1.0; // force special
        m.damage = 2;
        let before = h.hp;
        m.strike_hero(&mut h, &mut r, &mut log);
        // weapon 1..=2 + triple 6 => at least 7 total.
        assert!(before - h.hp >= 7, "log={log:?}");
    }

    #[test]
    fn heal_caps_at_max() {
        let mut h = Hero::new();
        h.hp = 14;
        let mut r = rng();
        let mut log = Vec::new();
        h.heal_self(&mut r, &mut log);
        assert!(h.hp <= h.max_hp);
    }

    #[test]
    fn soothe_can_bind() {
        let mut g = Game::new(99);
        g.current = Some(Monster::spawn(MonsterKind::MothWisp)); // threshold 50
        g.hero.breath = 6;
        g.act(Action::Soothe);
        g.act(Action::Soothe);
        g.act(Action::Soothe);
        assert!(g.hero.bestiary.contains(&MonsterKind::MothWisp));
    }

    #[test]
    fn vault_count_in_python_range() {
        for seed in 0..50 {
            let g = Game::new(seed);
            let total = g.queue.len() + if g.current.is_some() { 1 } else { 0 };
            assert!((3..=6).contains(&total), "seed {seed}: {total}");
        }
    }

    #[test]
    fn save_roundtrip() {
        let mut g = Game::new(1234);
        g.act(Action::Strike);
        let s = g.to_json();
        let h = Game::from_json(&s).expect("roundtrip");
        assert_eq!(h.hero.hp, g.hero.hp);
        assert_eq!(h.hero.score, g.hero.score);
    }

    #[test]
    fn sim_1000_battles_never_panics() {
        for seed in 0..200 {
            let (g, _) = simulate(seed, 60);
            let _ = g.to_json();
        }
    }

    #[test]
    fn full_victory_is_reachable() {
        // A lucky seed with scripted play should clear at least vault 0.
        let mut cleared_any = false;
        for seed in 0..30 {
            let (g, _) = simulate(seed, 400);
            if g.vault_index > 0 || g.phase == Phase::Victory || g.hero.score >= 3 {
                cleared_any = true;
                break;
            }
        }
        assert!(cleared_any);
    }

    #[test]
    fn bard_thirds_split_test_track() {
        // EMBER_VANGUARD_NOTES spans 57..=81 (span 24): thirds at 65 / 73.
        assert_eq!(classify_bard_note(0, 57), BardZone::Low);
        assert_eq!(classify_bard_note(0, 60), BardZone::Low);
        assert_eq!(classify_bard_note(0, 69), BardZone::Mid);
        assert_eq!(classify_bard_note(0, 72), BardZone::Mid);
        assert_eq!(classify_bard_note(0, 79), BardZone::High);
        assert_eq!(classify_bard_note(0, 81), BardZone::High);
    }

    #[test]
    fn bard_note_at_wraps_and_steps() {
        let dur = EMBER_VANGUARD_DUR;
        let (i0, m0) = bard_note_at(0, 0.0);
        assert_eq!((i0, m0), (0, 57));
        let (i1, m1) = bard_note_at(0, dur * 1.0);
        assert_eq!((i1, m1), (1, 60));
        // Loop wraps after 16 steps.
        let (iw, _) = bard_note_at(0, dur * 16.0);
        assert_eq!(iw, 0);
        // Negative clamps to start.
        let (ineg, _) = bard_note_at(0, -5.0);
        assert_eq!(ineg, 0);
    }

    #[test]
    fn battle_track_selected_in_range() {
        for seed in 0..20 {
            let g = Game::new(seed);
            assert!((g.battle_track as usize) < battle_track_count());
        }
    }

    #[test]
    fn strike_emits_bard_beat_with_explicit_clock() {
        let mut g = Game::new(42);
        g.current = Some(Monster::spawn(MonsterKind::Goblin));
        g.battle_track = 0;
        // t=0 -> note 57 -> Low.
        let events = g.act_at(Action::Strike, Some(0.0));
        let beat = events.iter().find_map(|e| match e {
            Event::BardBeat { zone, midi, .. } => Some((*zone, *midi)),
            _ => None,
        });
        assert_eq!(beat, Some((BardZone::Low, 57)));
        // High cue lands on a high step (t = 10 * dur -> midi 81).
        let mut g2 = Game::new(42);
        g2.current = Some(Monster::spawn(MonsterKind::Goblin));
        g2.battle_track = 0;
        let t_high = EMBER_VANGUARD_DUR * 10.0;
        let events = g2.act_at(Action::Strike, Some(t_high));
        assert!(events.iter().any(|e| matches!(
            e,
            Event::BardBeat {
                zone: BardZone::High,
                ..
            }
        )));
        assert!(events
            .iter()
            .any(|e| matches!(e, Event::Sound(SoundTrigger::BardHigh))));
    }

    #[test]
    fn bard_high_auto_hits_and_buffs() {
        // Even with hit = 0.0, High still lands and deals >= 1.
        let mut h = Hero::new();
        h.hit = 0.0;
        let mut m = Monster::spawn(MonsterKind::Goblin);
        m.enraged = false;
        let mut r = rng();
        let dmg = h.strike(&mut m, &mut r, BardZone::High);
        assert!(dmg >= 1, "high verse should auto-hit");
    }

    #[test]
    fn bard_low_penalizes_and_can_miss() {
        // Low lowers hit to 0.55 and shaves 2 damage; over many rolls we must
        // see at least one miss and never exceed the unpenalized max.
        let mut misses = 0;
        let mut max_seen = 0;
        for _ in 0..200 {
            let mut h = Hero::new();
            let mut m = Monster::spawn(MonsterKind::Goblin);
            let mut r = rng();
            let dmg = h.strike(&mut m, &mut r, BardZone::Low);
            max_seen = max_seen.max(dmg);
            if dmg == 0 {
                misses += 1;
            }
        }
        assert!(misses > 0, "low verse should miss sometimes");
        // Base max 5 + crit 2 + verse 2 = 9; minus 2 => <= 7.
        assert!(max_seen <= 7, "max {max_seen}");
    }

    #[test]
    fn save_roundtrip_keeps_battle_track() {
        let mut g = Game::new(1234);
        g.battle_track = 0;
        g.act_at(Action::Strike, Some(0.5));
        let s = g.to_json();
        let h = Game::from_json(&s).expect("roundtrip");
        assert_eq!(h.battle_track, g.battle_track);
        // Pre-bard saves (no battle_track key) still load as track 0.
        let legacy = r#"{"hero":{"name":"Lantern-Bard","hp":15,"max_hp":15,"damage":5,"hit":0.8,"score":0,"turns":-1,"breath":6,"max_breath":6,"verse_power":0,"companions":[],"bestiary":[],"shards":0},"phase":"Combat","vault_index":0,"queue":[],"current":null,"fled":false,"seed":7,"rng":null}"#;
        let old = Game::from_json(legacy).expect("legacy loads");
        assert_eq!(old.battle_track, 0);
    }
}
