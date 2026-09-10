//! 纯公历算法段：闰年（4/100/400）、非法日期拒绝、星期锚点、日计数、
//! ISO 往返与 CivilInstant 域校验。入口见 main.rs `gregorian_and_exchange_days`。

use crate::d;
use engine::calendar::{CivilDate, Weekday};

pub(crate) fn leap_year_rule() {
    // 运行区间内外的世纪边界：2000 闰（400 规则）、2100 不闰（100 规则）、2032 闰（4 规则）。
    assert!(
        CivilDate::from_ymd(2000, 2, 29).is_ok(),
        "2000-02-29 must exist"
    );
    assert!(
        CivilDate::from_ymd(2032, 2, 29).is_ok(),
        "2032-02-29 must exist"
    );
    // 2100 在算法验证窗内但非闰年：2 月 29 日必须被算法层拒绝。
    assert_eq!(
        CivilDate::from_ymd(2100, 2, 29).unwrap_err(),
        engine::calendar::CivilDateError::DayOutOfRange {
            year: 2100,
            month: 2,
            day: 29,
            days_in_month: 28
        }
    );
    assert!(d("2032-01-01").is_leap_year());
    assert!(d("2000-01-01").is_leap_year());
    assert!(!d("2100-01-01").is_leap_year(), "整百年非 400 倍数不闰");
    assert!(!d("1998-01-01").is_leap_year());
    assert!(
        CivilDate::from_ymd(2400, 1, 1).is_err(),
        "2400 超出算法验证窗"
    );
    assert!(
        CivilDate::from_ymd(1900, 1, 1).is_ok(),
        "1900 是算法验证窗下界"
    );
}

pub(crate) fn illegal_dates_rejected() {
    use engine::calendar::CivilDateError;
    assert!(matches!(
        CivilDate::from_ymd(2030, 2, 30),
        Err(CivilDateError::DayOutOfRange { day: 30, .. })
    ));
    assert!(matches!(
        CivilDate::from_ymd(2030, 4, 31),
        Err(CivilDateError::DayOutOfRange { .. })
    ));
    assert!(matches!(
        CivilDate::from_ymd(2030, 13, 1),
        Err(CivilDateError::MonthOutOfRange { month: 13 })
    ));
    assert!(matches!(
        CivilDate::from_ymd(2030, 0, 10),
        Err(CivilDateError::MonthOutOfRange { month: 0 })
    ));
    assert!(matches!(
        CivilDate::from_ymd(9999, 1, 1),
        Err(CivilDateError::YearOutOfRange { year: 9999 })
    ));
    assert!(matches!(
        CivilDate::from_ymd(1899, 12, 31),
        Err(CivilDateError::YearOutOfRange { year: 1899 })
    ));
    // 非法输入一律类型化错误，绝不静默钳位。
    for bad in [
        "2030-1-1",
        "2030/01/01",
        "2030-01-32",
        "",
        "2030-01",
        "xxxx-01-01",
    ] {
        assert!(
            CivilDate::from_iso(bad).is_err(),
            "from_iso({bad:?}) must fail"
        );
    }
    assert!(matches!(
        CivilDate::from_iso("2030-02-30"),
        Err(CivilDateError::DayOutOfRange { .. })
    ));
}

pub(crate) fn weekday_anchors() {
    // 独立已知锚点（跨来源核验：万年历常识 + HKO 对照表星期列）。
    assert_eq!(d("2000-01-01").weekday(), Weekday::Saturday);
    assert_eq!(d("1998-01-01").weekday(), Weekday::Thursday);
    assert_eq!(d("2030-01-01").weekday(), Weekday::Tuesday);
    assert_eq!(d("2032-02-29").weekday(), Weekday::Sunday);
    assert_eq!(d("2100-01-01").weekday(), Weekday::Friday);
    assert_eq!(d("2024-02-10").weekday(), Weekday::Saturday, "2024 春节");
    assert_eq!(d("2025-01-29").weekday(), Weekday::Wednesday, "2025 春节");
    assert!(Weekday::Saturday.is_weekend() && Weekday::Sunday.is_weekend());
    assert!(!Weekday::Monday.is_weekend());
}

pub(crate) fn day_counts_and_ordinals() {
    // 自然日计数：闰年 366、非闰 365；跨世纪算法一致。
    assert_eq!(d("2001-01-01").days_since(d("2000-01-01")), 366);
    assert_eq!(d("2000-01-01").days_since(d("2001-01-01")), -366);
    assert_eq!(
        d("2101-01-01").days_since(d("2100-01-01")),
        365,
        "2100 非闰"
    );
    assert_eq!(d("2000-03-01").day_of_year(), 61);
    assert_eq!(d("1998-12-31").day_of_year(), 365);
    assert_eq!(d("2032-12-31").day_of_year(), 366);
    // 相邻日推进与回退。
    assert_eq!(d("2030-01-31").next().unwrap(), d("2030-02-01"));
    assert_eq!(d("2030-03-01").prev().unwrap(), d("2030-02-28"));
    assert_eq!(d("2000-02-28").next().unwrap(), d("2000-02-29"));
    assert_eq!(
        d("2100-02-28").next().unwrap(),
        d("2100-03-01"),
        "2100 无 2-29"
    );
}

pub(crate) fn iso_roundtrip_and_civil_instant() {
    assert_eq!(d("2030-01-01").to_iso(), "2030-01-01");
    assert_eq!(CivilDate::from_iso("1998-01-01").unwrap().year(), 1998);
    assert_eq!(d("2005-06-07").month(), 6);
    assert_eq!(d("2005-06-07").day(), 7);

    use engine::calendar::{CivilDateError, CivilInstant};
    let date = d("2030-01-01");
    assert_eq!(CivilInstant::new(date, 0).unwrap().second_of_day(), 0);
    assert_eq!(
        CivilInstant::new(date, 86399).unwrap().second_of_day(),
        86399
    );
    assert!(matches!(
        CivilInstant::new(date, 86400),
        Err(CivilDateError::TimeComponent { value: 86400, .. })
    ));
    let inst = CivilInstant::from_hms(date, 18, 0, 0).unwrap();
    assert_eq!(inst.second_of_day(), 18 * 3600);
    assert_eq!(inst.date(), date);
    assert!(CivilInstant::from_hms(date, 24, 0, 0).is_err());
    assert!(CivilInstant::from_hms(date, 12, 60, 0).is_err());
    // serde 往返。
    let json = serde_json::to_string(&inst).unwrap();
    assert_eq!(serde_json::from_str::<CivilInstant>(&json).unwrap(), inst);
    let date_json = serde_json::to_string(&date).unwrap();
    assert_eq!(date_json, "\"2030-01-01\"");
    assert_eq!(serde_json::from_str::<CivilDate>(&date_json).unwrap(), date);
}
