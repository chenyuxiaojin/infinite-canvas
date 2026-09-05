use crate::{db, prompts, routes::AppState, settings};
use chrono::Local;
use croner::Cron;
use std::{str::FromStr, time::Duration};

pub async fn run(state: AppState) {
    loop {
        let saved = db::connect(&state.database).and_then(|db| settings::read(&db, "private"));
        let setting = match saved {
            Ok(saved) => saved["promptSync"].clone(),
            Err(_) => {
                eprintln!("prompt-sync: could not read local settings");
                tokio::time::sleep(Duration::from_secs(60)).await;
                continue;
            }
        };
        if setting["enabled"].as_bool() == Some(false) {
            tokio::time::sleep(Duration::from_secs(60)).await;
            continue;
        }
        let pattern = setting["cron"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or("0 0 * * *");
        let next = Cron::from_str(pattern)
            .ok()
            .and_then(|cron| cron.find_next_occurrence(&Local::now(), false).ok());
        let Some(next) = next else {
            eprintln!("prompt-sync: invalid local schedule");
            tokio::time::sleep(Duration::from_secs(60)).await;
            continue;
        };
        // Check settings while waiting; a changed or disabled schedule takes effect without restart.
        let wait = (next - Local::now()).to_std().unwrap_or(Duration::ZERO);
        if wait > Duration::from_secs(60) {
            tokio::time::sleep(Duration::from_secs(60)).await;
            continue;
        }
        tokio::time::sleep(wait).await;
        let same = db::connect(&state.database)
            .and_then(|db| settings::read(&db, "private"))
            .is_ok_and(|v| v["promptSync"] == setting);
        if !same {
            continue;
        }
        let guard = state.sync_lock.clone().lock_owned().await;
        let path = state.database.clone();
        match tokio::task::spawn_blocking(move || {
            let _guard = guard;
            prompts::sync_all(&path)
        })
        .await
        {
            Ok(Ok(results)) => {
                let failures = results
                    .as_array()
                    .map(|xs| xs.iter().filter(|x| x.get("error").is_some()).count())
                    .unwrap_or_default();
                eprintln!("prompt-sync: completed, failed sources={failures}");
            }
            _ => eprintln!("prompt-sync: failed; existing catalogs retained"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn standard_five_field_schedule_keeps_local_midnight() {
        use chrono::{TimeZone, Timelike};
        let date = Local.with_ymd_and_hms(2026, 9, 5, 12, 0, 0).unwrap();
        let next = Cron::from_str("0 0 * * *")
            .unwrap()
            .find_next_occurrence(&date, false)
            .unwrap();
        assert_eq!(next.hour(), 0);
        assert_eq!(next.minute(), 0);
        assert!(next > date);
    }
}
