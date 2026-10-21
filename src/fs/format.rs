//! String formatting of file attributes, ported from lfm's `utils.py`
//! (size_human, time2str, perms2str, str2perms, rdev2str, owner2str, group2str, num2str).

/// `ls -h` style size, 1024-based with one decimal below 10 (yafm extra; lfm
/// showed bytes, switching to K/M only for big files).
pub fn size_human(size: u64) -> String {
    const UNITS: [char; 6] = ['K', 'M', 'G', 'T', 'P', 'E'];
    if size < 1024 {
        return format!("{size}B");
    }
    let (mut v, mut u) = (size as f64 / 1024.0, 0);
    while v.round() >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if (v * 10.0).round() < 100.0 {
        format!("{v:.1}{}", UNITS[u])
    } else {
        format!("{v:.0}{}", UNITS[u])
    }
}

/// Six months, the window in which lfm shows the time instead of the year.
const RECENT_SECS: i64 = 15_552_000;

pub fn time2str_at<Tz: chrono::TimeZone>(t: i64, now: i64, short: bool, tz: &Tz) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let fmt = match ((now - t).abs() < RECENT_SECS, short) {
        (true, true) => "%d %b %H:%M",
        (true, false) => "%a %b %d %H:%M",
        (false, true) => "%d %b  %Y",
        (false, false) => "%a  %d %b %Y",
    };
    match tz.timestamp_opt(t, 0).single() {
        Some(dt) => dt.format(fmt).to_string(),
        None => t.to_string(),
    }
}

pub fn time2str(t: i64, short: bool) -> String {
    time2str_at(t, chrono::Utc::now().timestamp(), short, &chrono::Local)
}

/// Like Python `stat.filemode(mode)[1:].lower()`.
pub fn perms2str(mode: u32) -> String {
    perms_chars(mode).to_lowercase()
}

/// `stat.filemode(mode)[1:]`: `s`/`t` with execute, `S`/`T` without.
fn perms_chars(mode: u32) -> String {
    let bit = |mask: u32, c: char| if mode & mask != 0 { c } else { '-' };
    let special = |x: u32, s: u32, c: char| match (mode & x != 0, mode & s != 0) {
        (true, true) => c,
        (false, true) => c.to_ascii_uppercase(),
        (true, false) => 'x',
        (false, false) => '-',
    };
    let s: String = [
        bit(0o400, 'r'),
        bit(0o200, 'w'),
        special(0o100, 0o4000, 's'),
        bit(0o040, 'r'),
        bit(0o020, 'w'),
        special(0o010, 0o2000, 's'),
        bit(0o004, 'r'),
        bit(0o002, 'w'),
        special(0o001, 0o1000, 't'),
    ]
    .iter()
    .collect();
    s
}

/// Inverse of `perms2str`/`perms_display`. Unlike lfm, `s`/`t` also set the
/// matching x bit (that is what they mean); `S`/`T` set only the special bit.
pub fn str2perms(perms: &str) -> u32 {
    let mut ps = 0;
    for (i, p) in perms.chars().rev().enumerate() {
        let shift = 3 * (i as u32 / 3);
        match (p, i) {
            ('x', _) => ps += 1 << shift,
            ('w', _) => ps += 2 << shift,
            ('r', _) => ps += 4 << shift,
            ('t', 0) => ps += 0o1000 + 1,
            ('s', 6) => ps += 0o4000 + 0o100,
            ('s', 3) => ps += 0o2000 + 0o010,
            ('T', 0) => ps += 0o1000,
            ('S', 6) => ps += 0o4000,
            ('S', 3) => ps += 0o2000,
            _ => {}
        }
    }
    ps
}

/// Like `perms2str`, but keeps `S`/`T` for set-id/sticky without execute.
pub fn perms_display(mode: u32) -> String {
    perms_chars(mode)
}

pub fn rdev2str(major: u64, minor: u64) -> String {
    format!("{:>7}", format!("{major},{minor}"))
}

pub fn owner2str(uid: u32) -> String {
    match nix::unistd::User::from_uid(nix::unistd::Uid::from_raw(uid)) {
        Ok(Some(u)) => u.name,
        _ => uid.to_string(),
    }
}

pub fn group2str(gid: u32) -> String {
    match nix::unistd::Group::from_gid(nix::unistd::Gid::from_raw(gid)) {
        Ok(Some(g)) => g.name,
        _ => gid.to_string(),
    }
}

pub fn num2str(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_sizes() {
        for (n, s) in [
            (0, "0B"),
            (1023, "1023B"),
            (1024, "1.0K"),
            (1536, "1.5K"),
            (10 * 1024, "10K"),
            (777_777, "760K"),
            (1_048_575, "1.0M"),
            (1_000_000_000, "954M"),
            (5 << 30, "5.0G"),
            (3 << 40, "3.0T"),
            (u64::MAX, "16E"),
        ] {
            assert_eq!(size_human(n), s, "{n}");
        }
    }

    #[test]
    fn times_recent_and_old() {
        let now = 1_700_000_000; // 2023-11-14 22:13:20 UTC
        assert_eq!(
            time2str_at(now - 3600, now, true, &chrono::Utc),
            "14 Nov 21:13"
        );
        assert_eq!(
            time2str_at(now - 3600, now, false, &chrono::Utc),
            "Tue Nov 14 21:13"
        );
        assert_eq!(
            time2str_at(now - 20_000_000, now, true, &chrono::Utc),
            "28 Mar  2023"
        );
        assert_eq!(
            time2str_at(now - 20_000_000, now, false, &chrono::Utc),
            "Tue  28 Mar 2023"
        );
    }

    #[test]
    fn perms() {
        assert_eq!(perms2str(0o755), "rwxr-xr-x");
        assert_eq!(perms2str(0o4755), "rwsr-xr-x");
        assert_eq!(perms2str(0o1777), "rwxrwxrwt");
        assert_eq!(perms2str(0o4644), "rwsr--r--"); // "S" lowercased, as lfm shows it
        assert_eq!(str2perms("rwxr-xr-x"), 0o755);
        assert_eq!(str2perms("rwsr-xr-x"), 0o4755);
        assert_eq!(str2perms("rwxrwsr-x"), 0o2775);
        assert_eq!(str2perms("rwxrwxrwt"), 0o1777);
    }

    #[test]
    fn misc() {
        assert_eq!(rdev2str(8, 1), "    8,1");
        assert_eq!(num2str(1234567), "1,234,567");
        assert_eq!(num2str(999), "999");
        assert_eq!(owner2str(0), "root");
        assert_eq!(group2str(0), "root");
        assert_eq!(owner2str(4_000_000_000), "4000000000");
    }

    #[test]
    fn display_keeps_capital_s() {
        assert_eq!(perms_display(0o4644), "rwSr--r--");
        assert_eq!(str2perms("rwSr--r--"), 0o4644);
        assert_eq!(str2perms(&perms_display(0o1754)), 0o1754);
        assert_eq!(str2perms(&perms_display(0o2745)), 0o2745);
        assert_eq!(perms_display(0o755), "rwxr-xr-x");
    }
}
