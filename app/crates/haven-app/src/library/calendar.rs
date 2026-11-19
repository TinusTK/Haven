//! The Calendar page: when the films and episodes of the titles on the
//! Watchlist come out, a month at a time, in the user's own time zone.

use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    future::Future,
    pin::Pin,
    sync::Arc,
};

use chrono::{DateTime, Datelike, Days, Local, Months, NaiveDate, NaiveTime, TimeZone};
use haven_addons::MetaDetail;
use slint::{ModelRc, VecModel};
use tokio::sync::Semaphore;

use super::{Library, on_ui};
use crate::{AgendaRow, CalendarDay, CalendarEntry, Poster};

/// Titles whose details are asked for at the same time.
const TITLES_AT_ONCE: usize = 2;
/// Releases named in one day's square; the rest are counted.
const SHOWN_PER_DAY: usize = 2;

const MONTHS: [&str; 12] = [
    "January", "February", "March", "April", "May", "June", "July", "August", "September",
    "October", "November", "December",
];
const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/// One film or episode coming out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Release {
    day: NaiveDate,
    /// The local time of day, when the source gives a time.
    time: Option<NaiveTime>,
    /// "S3 E5 · Its name" for an episode; empty for a film.
    label: String,
    /// The episode's own id, to open it by; empty for a film.
    video: String,
    /// "S01E05" for an episode; empty for a film.
    number: String,
    /// The episode's name, if it has one.
    name: String,
}

/// A title on the Watchlist: its kind and id.
type Title = (String, String);

pub(super) struct Calendar {
    /// A day in the month or week showing.
    anchor: Cell<NaiveDate>,
    /// A week at a time instead of a month.
    week: Cell<bool>,
    releases: RefCell<HashMap<Title, Vec<Release>>>,
    /// Titles asked for already (answered or not).
    asked: RefCell<HashSet<Title>>,
    waiting: Cell<usize>,
    /// Each title's wide picture (else its poster), for Home's Upcoming.
    pictures: RefCell<HashMap<Title, String>>,
    /// The page has been opened, so it is kept up to date.
    opened: Cell<bool>,
    gate: Arc<Semaphore>,
}

impl Default for Calendar {
    fn default() -> Self {
        let today = Local::now().date_naive();
        Self {
            anchor: Cell::new(today),
            week: Cell::new(false),
            releases: RefCell::default(),
            asked: RefCell::default(),
            waiting: Cell::new(0),
            pictures: RefCell::default(),
            opened: Cell::new(false),
            gate: Arc::new(Semaphore::new(TITLES_AT_ONCE)),
        }
    }
}

/// When something released at `released` (an ISO date, or a moment
/// with a time zone) comes out in the time zone `zone`. A moment is
/// moved to that zone, so an evening in America is the next day in
/// Australia. A bare date, or midnight UTC (how sources write a date
/// they have no time for), stays that date: there is no time to move.
pub(super) fn local_release<Tz: TimeZone>(
    released: &str,
    zone: &Tz,
) -> Option<(NaiveDate, Option<NaiveTime>)> {
    let released = released.trim();
    if let Ok(moment) = DateTime::parse_from_rfc3339(released) {
        let utc = moment.naive_utc();
        if utc.time() == NaiveTime::MIN {
            return Some((utc.date(), None));
        }
        let local = moment.with_timezone(zone);
        return Some((local.date_naive(), Some(local.time())));
    }
    NaiveDate::parse_from_str(released.get(..10)?, "%Y-%m-%d").ok().map(|day| (day, None))
}

/// What a title's details say comes out, and when, in `zone`.
pub(super) fn releases_of<Tz: TimeZone>(meta: &MetaDetail, zone: &Tz) -> Vec<Release> {
    let mut releases: Vec<Release> = meta
        .videos
        .iter()
        // Specials (season 0) are left out: sources list making-of
        // pieces there on the same day as the real episodes.
        .filter(|video| video.season != Some(0))
        .filter_map(|video| {
            let (day, time) = local_release(video.released.as_deref()?, zone)?;
            let number = match (video.season, video.episode) {
                (Some(season), Some(episode)) => format!("S{season} E{episode}"),
                (None, Some(episode)) => format!("E{episode}"),
                _ => String::new(),
            };
            let label = [number.as_str(), video.title.as_deref().unwrap_or_default()]
                .into_iter()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join(" · ");
            let short = match (video.season, video.episode) {
                (Some(season), Some(episode)) => format!("S{season:02}E{episode:02}"),
                (None, Some(episode)) => format!("E{episode:02}"),
                _ => String::new(),
            };
            Some(Release {
                day,
                time,
                label,
                video: video.id.clone(),
                number: short,
                name: video.title.clone().unwrap_or_default(),
            })
        })
        .collect();
    // The same episode listed twice (some sources repeat it) shows once.
    let mut seen = HashSet::new();
    releases.retain(|release| seen.insert((release.day, release.label.clone())));
    // A film, or a series with no episodes listed: the title's own date.
    if releases.is_empty() {
        if let Some((day, time)) =
            meta.released.as_deref().and_then(|released| local_release(released, zone))
        {
            releases.push(Release {
                day,
                time,
                label: String::new(),
                video: String::new(),
                number: String::new(),
                name: String::new(),
            });
        }
    }
    releases
}

/// The 7 days of the week (Monday first) that holds `day`.
pub(super) fn week_days(day: NaiveDate) -> Vec<NaiveDate> {
    let start = day - Days::new(day.weekday().num_days_from_monday() as u64);
    (0..7).map(|at| start + Days::new(at)).collect()
}

/// The 42 days a month's page shows: whole weeks from the Monday on or
/// before the 1st.
pub(super) fn month_days(year: i32, month: u32) -> Vec<NaiveDate> {
    let Some(first) = NaiveDate::from_ymd_opt(year, month, 1) else { return Vec::new() };
    let start = first - Days::new(first.weekday().num_days_from_monday() as u64);
    (0..42).map(|day| start + Days::new(day)).collect()
}

impl Library {
    /// Fetches a title's details from wherever it came from: its media
    /// server, TMDB, or the first add-on that knows it.
    pub(super) fn details_of(
        &self,
        kind: &str,
        id: &str,
    ) -> Option<Pin<Box<dyn Future<Output = Result<MetaDetail, String>> + Send>>> {
        if let Some((server, item)) = self.server_item(id) {
            let client = self.server_client.clone();
            return Some(Box::pin(async move {
                client.meta(&server, &item).await.map_err(|err| err.to_string())
            }));
        }
        if let Some((series, number)) = haven_metadata::split_id(id) {
            let tmdb = self.tmdb()?;
            return Some(Box::pin(async move {
                tmdb.title(series, number).await.map_err(|err| err.to_string())
            }));
        }
        let sources: Vec<String> = self
            .installed
            .borrow()
            .iter()
            .filter(|addon| addon.manifest.provides("meta", kind, id))
            .map(|addon| addon.url.clone())
            .collect();
        if sources.is_empty() {
            return None;
        }
        let (addons, kind, id) = (self.addons.clone(), kind.to_owned(), id.to_owned());
        Some(Box::pin(async move {
            let mut found = Err("no add-on has it".to_owned());
            for url in sources {
                match addons.meta(&url, &kind, &id).await {
                    Ok(meta) => return Ok(meta),
                    Err(err) => found = Err(err.to_string()),
                }
            }
            found
        }))
    }

    /// Titles whose dates have been asked for, and how many have not answered.
    pub(super) fn calendar_load(&self) -> (usize, usize) {
        (self.calendar.asked.borrow().len(), self.calendar.waiting.get())
    }

    pub(super) fn start_calendar(self: &std::rc::Rc<Self>, ui: &crate::AppWindow) {
        ui.on_calendar_shown({
            let library = self.clone();
            move || {
                library.calendar.opened.set(true);
                library.refresh_calendar();
            }
        });
        ui.on_calendar_step({
            let library = self.clone();
            move |step| library.step_calendar(step)
        });
        ui.on_calendar_week({
            let library = self.clone();
            move |week| {
                library.calendar.week.set(week);
                library.show_calendar();
            }
        });
        ui.on_calendar_open({
            let library = self.clone();
            move |entry| {
                // An episode opens on that episode, with its places to
                // watch; a film on its own page.
                library.open_title(Poster {
                    id: entry.id,
                    kind: entry.kind,
                    title: entry.title,
                    video: entry.video,
                    ..Poster::default()
                })
            }
        });
        ui.set_calendar_zone(
            format!("Times are in your local time (UTC{}).", Local::now().offset()).into(),
        );
        // Home's Upcoming row needs the dates too: look them up at start.
        self.calendar.opened.set(true);
        self.refresh_calendar();
    }

    /// Moves a month (or week) back (-1) or on (1), or to today (0).
    fn step_calendar(&self, step: i32) {
        let today = Local::now().date_naive();
        let shown = self.calendar.anchor.get();
        let week = self.calendar.week.get();
        let next = match step {
            0 => Some(today),
            step if step < 0 && week => shown.checked_sub_days(Days::new(7)),
            _ if week => shown.checked_add_days(Days::new(7)),
            step if step < 0 => shown.with_day(1).and_then(|d| d.checked_sub_months(Months::new(1))),
            _ => shown.with_day(1).and_then(|d| d.checked_add_months(Months::new(1))),
        };
        if let Some(next) = next {
            self.calendar.anchor.set(next);
        }
        self.show_calendar();
    }

    /// Brings the calendar in line with the Watchlist: asks for the
    /// details of titles not asked for yet, forgets titles taken off.
    /// Does nothing until the page has been opened once.
    pub(super) fn refresh_calendar(&self) {
        if !self.calendar.opened.get() {
            return;
        }
        let saved = self.store.watchlist().unwrap_or_default();
        let listed: HashSet<Title> =
            saved.iter().map(|saved| (saved.kind.clone(), saved.title_id.clone())).collect();
        self.calendar.releases.borrow_mut().retain(|title, _| listed.contains(title));
        self.calendar.asked.borrow_mut().retain(|title| listed.contains(title));
        for title in listed {
            if !self.calendar.asked.borrow_mut().insert(title.clone()) {
                continue;
            }
            let Some(details) = self.details_of(&title.0, &title.1) else { continue };
            self.calendar.waiting.set(self.calendar.waiting.get() + 1);
            let gate = self.calendar.gate.clone();
            self.tasks.spawn(async move {
                let found = match gate.acquire().await {
                    Ok(_turn) => details.await,
                    Err(_) => Err("stopped".to_owned()),
                };
                let releases = found.as_ref().map(|meta| releases_of(meta, &Local)).ok();
                let picture = found.as_ref().ok().and_then(|meta| meta.background.clone().or(meta.poster.clone()));
                if let Err(err) = &found {
                    haven_core::diag::note(&format!("calendar: no dates for a title ({err})"));
                }
                on_ui(move |library| {
                    let calendar = &library.calendar;
                    calendar.waiting.set(calendar.waiting.get().saturating_sub(1));
                    // Taken off the Watchlist meanwhile: not wanted.
                    if calendar.asked.borrow().contains(&title) {
                        match releases {
                            Some(releases) => {
                                if let Some(picture) = picture {
                                    calendar.pictures.borrow_mut().insert(title.clone(), picture);
                                }
                                calendar.releases.borrow_mut().insert(title, releases);
                            }
                            // Asked again the next time the page opens.
                            None => {
                                calendar.asked.borrow_mut().remove(&title);
                            }
                        }
                    }
                    library.show_calendar();
                });
            });
        }
        self.show_calendar();
    }

    /// Fills the page for the month showing.
    fn show_calendar(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let anchor = self.calendar.anchor.get();
        let (year, month) = (anchor.year(), anchor.month());
        let week = self.calendar.week.get();
        let today = Local::now().date_naive();
        let names: HashMap<Title, String> = self
            .store
            .watchlist()
            .unwrap_or_default()
            .into_iter()
            .map(|saved| ((saved.kind, saved.title_id), saved.title))
            .collect();

        // Everything coming out, by day, in order of time then name.
        let mut by_day: HashMap<NaiveDate, Vec<(Option<NaiveTime>, CalendarEntry)>> = HashMap::new();
        for (title, releases) in self.calendar.releases.borrow().iter() {
            let Some(name) = names.get(title) else { continue };
            for release in releases {
                by_day.entry(release.day).or_default().push((
                    release.time,
                    CalendarEntry {
                        kind: title.0.as_str().into(),
                        id: title.1.as_str().into(),
                        title: name.as_str().into(),
                        label: release.label.as_str().into(),
                        video: release.video.as_str().into(),
                        time: release
                            .time
                            .map(|time| time.format("%H:%M").to_string())
                            .unwrap_or_default()
                            .into(),
                    },
                ));
            }
        }
        for entries in by_day.values_mut() {
            entries.sort_by(|a, b| {
                (a.0.is_none(), a.0, &a.1.title, &a.1.label)
                    .cmp(&(b.0.is_none(), b.0, &b.1.title, &b.1.label))
            });
        }

        let days = if week { week_days(anchor) } else { month_days(year, month) };
        // A week's squares are tall enough to name everything.
        let shown_per_day = if week { usize::MAX } else { SHOWN_PER_DAY };
        let squares: Vec<CalendarDay> = days
            .iter()
            .map(|day| {
                let entries = by_day.get(day).map(Vec::as_slice).unwrap_or_default();
                CalendarDay {
                    number: day.day() as i32,
                    in_month: week || day.month() == month,
                    today: *day == today,
                    more: entries.len().saturating_sub(shown_per_day) as i32,
                    entries: ModelRc::new(VecModel::from(
                        entries
                            .iter()
                            .take(shown_per_day)
                            .map(|(_, entry)| entry.clone())
                            .collect::<Vec<_>>(),
                    )),
                }
            })
            .collect();
        let mut agenda: Vec<AgendaRow> = Vec::new();
        for day in days.iter().filter(|day| week || day.month() == month) {
            for (at, (_, entry)) in by_day.get(day).into_iter().flatten().enumerate() {
                agenda.push(AgendaRow {
                    heading: if at == 0 {
                        let weekday = WEEKDAYS[day.weekday().num_days_from_monday() as usize];
                        format!("{weekday} {}", day.day()).into()
                    } else {
                        "".into()
                    },
                    today: *day == today,
                    past: *day < today,
                    entry: entry.clone(),
                });
            }
        }
        let note = if names.is_empty() {
            "Add films and series to your Watchlist to see here when they come out."
        } else if self.calendar.waiting.get() > 0 {
            "Looking up dates…"
        } else if agenda.is_empty() && week {
            "Nothing on your Watchlist comes out this week."
        } else if agenda.is_empty() {
            "Nothing on your Watchlist comes out this month."
        } else {
            ""
        };
        let month_name = |day: NaiveDate| MONTHS[day.month0() as usize];
        let title = if week {
            let (first, last) = (days[0], days[6]);
            if first.month() == last.month() {
                format!("{} – {} {} {}", first.day(), last.day(), month_name(last), last.year())
            } else {
                format!("{} {} – {} {} {}", first.day(), &month_name(first)[..3], last.day(), &month_name(last)[..3], last.year())
            }
        } else {
            format!("{} {year}", month_name(anchor))
        };
        ui.set_calendar_title(title.into());
        ui.set_calendar_is_week(week);
        ui.set_calendar_days(ModelRc::new(VecModel::from(squares)));
        ui.set_calendar_agenda(ModelRc::new(VecModel::from(agenda)));
        ui.set_calendar_note(note.into());
        drop(ui);
        self.fill_upcoming(&names, today);
    }

    /// Home's Upcoming row: what comes out from today on, soonest first.
    fn fill_upcoming(&self, names: &HashMap<Title, String>, today: NaiveDate) {
        let releases = self.calendar.releases.borrow();
        let pictures = self.calendar.pictures.borrow();
        let mut coming: Vec<(&Title, &Release)> = releases
            .iter()
            .flat_map(|(title, list)| list.iter().map(move |release| (title, release)))
            .filter(|(_, release)| release.day >= today)
            .collect();
        coming.sort_by(|a, b| (a.1.day, a.1.time, &a.0.1).cmp(&(b.1.day, b.1.time, &b.0.1)));
        // One card a show (user: two episodes of one show clog the row):
        // its next release; more of it the same day widen the badge.
        let mut last_same_day: HashMap<&Title, &Release> = HashMap::new();
        for (title, release) in &coming {
            let first = coming.iter().find(|(other, _)| other == title).map(|(_, r)| r.day);
            if Some(release.day) == first {
                last_same_day.insert(*title, *release);
            }
        }
        let mut seen: HashSet<&Title> = HashSet::new();
        coming.retain(|(title, _)| seen.insert(*title));
        let cards = coming
            .into_iter()
            .filter_map(|(title, release)| {
                let last = last_same_day.get(title).copied().filter(|last| last.number != release.number);
                let name = names.get(title)?;
                let days = (release.day - today).num_days();
                let when = match days {
                    0 => "Today".to_owned(),
                    1 => "Tomorrow".to_owned(),
                    2..=6 => format!("In {days} days"),
                    _ => format!("{} {}", &MONTHS[release.day.month0() as usize][..3], release.day.day()),
                };
                let card = crate::UpcomingCard {
                    id: title.1.as_str().into(),
                    kind: title.0.as_str().into(),
                    video: release.video.as_str().into(),
                    title: name.as_str().into(),
                    episode: if last.is_some() {
                        "New episodes".into()
                    } else if release.name.is_empty() {
                        if release.number.is_empty() { "Film".into() } else { release.number.as_str().into() }
                    } else {
                        release.name.as_str().into()
                    },
                    badge: match last {
                        // "S03E07–E08"
                        Some(last) if last.number.len() > 3 => {
                            format!("{}–{}", release.number, &last.number[last.number.len().saturating_sub(3)..]).into()
                        }
                        _ => release.number.as_str().into(),
                    },
                    when: when.into(),
                    soon: days < 7,
                    ..Default::default()
                };
                Some((card, pictures.get(title).cloned()))
            })
            .take(12)
            .collect();
        drop((releases, pictures));
        self.show_upcoming(cards);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    #[test]
    fn a_moment_moves_to_the_local_day_and_a_bare_date_stays() {
        let sydney = FixedOffset::east_opt(11 * 3600).unwrap();
        let new_york = FixedOffset::west_opt(5 * 3600).unwrap();
        let day = |y, m, d| NaiveDate::from_ymd_opt(y, m, d).unwrap();
        let time = |h, m| NaiveTime::from_hms_opt(h, m, 0).unwrap();
        // 9 pm on the 11th in New York is 02:00 UTC on the 12th.
        let aired = "2026-11-12T02:00:00.000Z";
        assert_eq!(local_release(aired, &new_york), Some((day(2026, 11, 11), Some(time(21, 0)))));
        assert_eq!(local_release(aired, &sydney), Some((day(2026, 11, 12), Some(time(13, 0)))));
        // The same moment written with its own offset.
        assert_eq!(
            local_release("2026-11-11T21:00:00-05:00", &sydney),
            Some((day(2026, 11, 12), Some(time(13, 0))))
        );
        // No time known: the date as given, wherever you are.
        assert_eq!(local_release("2026-11-11", &sydney), Some((day(2026, 11, 11), None)));
        assert_eq!(
            local_release("2026-11-11T00:00:00.000Z", &new_york),
            Some((day(2026, 11, 11), None))
        );
        assert_eq!(local_release("soon", &sydney), None);
    }

    #[test]
    fn a_month_page_is_six_weeks_from_a_monday() {
        let days = month_days(2026, 11);
        assert_eq!(days.len(), 42);
        // 1 November 2026 is a Sunday: the page starts Monday 26 October.
        assert_eq!(days[0], NaiveDate::from_ymd_opt(2026, 10, 26).unwrap());
        assert_eq!(days[6], NaiveDate::from_ymd_opt(2026, 11, 1).unwrap());
        assert_eq!(days[41], NaiveDate::from_ymd_opt(2026, 12, 6).unwrap());
    }

    #[test]
    fn episodes_and_films_give_releases() {
        let zone = FixedOffset::east_opt(0).unwrap();
        let series: MetaDetail = serde_json::from_value(serde_json::json!({
            "id": "tt1", "type": "series", "name": "A Show", "released": "2020-01-01",
            "videos": [
                {"id": "tt1:3:5", "title": "Five", "season": 3, "episode": 5, "released": "2026-11-18T08:30:00.000Z"},
                {"id": "tt1:3:6", "season": 3, "episode": 6}
            ]
        }))
        .unwrap();
        let releases = releases_of(&series, &zone);
        assert_eq!(releases.len(), 1);
        assert_eq!(releases[0].label, "S3 E5 · Five");
        let doubled: MetaDetail = serde_json::from_value(serde_json::json!({
            "id": "tt3", "type": "series", "name": "B",
            "videos": [
                {"id": "tt3:0:31", "title": "The Making Of Episode 1", "season": 0, "episode": 31, "released": "2026-11-11T11:00:00.000Z"},
                {"id": "tt3:3:1", "title": "One", "season": 3, "episode": 1, "released": "2026-11-11T11:00:00.000Z"},
                {"id": "tt3:3:1", "title": "One", "season": 3, "episode": 1, "released": "2026-11-11T11:00:00.000Z"}
            ]
        }))
        .unwrap();
        let once = releases_of(&doubled, &zone);
        assert_eq!(once.len(), 1);
        assert_eq!(once[0].label, "S3 E1 · One");
        assert_eq!(week_days(NaiveDate::from_ymd_opt(2026, 11, 19).unwrap())[0],
            NaiveDate::from_ymd_opt(2026, 11, 16).unwrap());
        assert_eq!(releases[0].video, "tt1:3:5");
        assert_eq!(releases[0].time, NaiveTime::from_hms_opt(8, 30, 0));
        let film: MetaDetail = serde_json::from_value(serde_json::json!({
            "id": "tt2", "type": "movie", "name": "A Film", "released": "2026-12-25T00:00:00.000Z"
        }))
        .unwrap();
        let releases = releases_of(&film, &zone);
        assert_eq!(releases.len(), 1);
        assert_eq!((releases[0].day, releases[0].time, releases[0].label.as_str()),
            (NaiveDate::from_ymd_opt(2026, 12, 25).unwrap(), None, ""));
    }
}
