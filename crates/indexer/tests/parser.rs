//! Release-name parser tables. Names follow public scene/P2P naming conventions; titles are well-known public works or invented. Each table reports every mismatch at once.
#![allow(clippy::type_complexity)] // case tables
use nocturne_indexer::Quality::{self, *};
use nocturne_indexer::*;

fn report(fails: Vec<String>, total: usize) {
    assert!(
        fails.is_empty(),
        "{} of {total} failed:\n{}",
        fails.len(),
        fails.join("\n")
    );
}

#[test]
fn movies() {
    let cases: &[(&str, &str, Option<u16>, Quality)] = &[
        (
            "The.Matrix.1999.1080p.BluRay.x264-GROUP",
            "The Matrix",
            Some(1999),
            Bluray1080p,
        ),
        (
            "The.Matrix.1999.720p.BluRay.x264-GROUP",
            "The Matrix",
            Some(1999),
            Bluray720p,
        ),
        (
            "The Matrix (1999) 2160p UHD BluRay x265 10bit HDR DTS-HD MA 5.1-GROUP",
            "The Matrix",
            Some(1999),
            Bluray2160p,
        ),
        (
            "The.Matrix.1999.2160p.UHD.BluRay.REMUX.HDR.HEVC.Atmos-GROUP",
            "The Matrix",
            Some(1999),
            Remux2160p,
        ),
        (
            "The.Matrix.1999.1080p.BluRay.REMUX.AVC.DTS-HD.MA.5.1-GROUP",
            "The Matrix",
            Some(1999),
            Remux1080p,
        ),
        (
            "The.Matrix.1999.1080p.BDRemux.AVC-GROUP",
            "The Matrix",
            Some(1999),
            Remux1080p,
        ),
        (
            "Inception.2010.1080p.WEB-DL.DD5.1.H264-GROUP",
            "Inception",
            Some(2010),
            WebDl1080p,
        ),
        (
            "Inception.2010.1080p.WEBRip.x264.AAC-GROUP",
            "Inception",
            Some(2010),
            WebRip1080p,
        ),
        (
            "Inception.2010.720p.WEB-DL.DD5.1.H264-GROUP",
            "Inception",
            Some(2010),
            WebDl720p,
        ),
        (
            "Inception.2010.2160p.WEB-DL.DDP5.1.Atmos.DV.HDR.H.265-GROUP",
            "Inception",
            Some(2010),
            WebDl2160p,
        ),
        (
            "Inception 2010 2160p AMZN WEB-DL DDP5.1 HDR10+ HEVC-GROUP",
            "Inception",
            Some(2010),
            WebDl2160p,
        ),
        (
            "Inception.2010.1080p.AMZN.WEBRip.DDP5.1.x264-GROUP",
            "Inception",
            Some(2010),
            WebRip1080p,
        ),
        (
            "Inception.2010.1080p.NF.WEB.H264-GROUP",
            "Inception",
            Some(2010),
            WebDl1080p,
        ),
        (
            "Inception.2010.480p.WEB-DL.x264-GROUP",
            "Inception",
            Some(2010),
            WebDl480p,
        ),
        (
            "Inception.2010.WEBRip.XviD-GROUP",
            "Inception",
            Some(2010),
            WebRip480p,
        ),
        (
            "Inception.2010.DVDRip.XviD-GROUP",
            "Inception",
            Some(2010),
            Dvd,
        ),
        (
            "Inception.2010.DVD9.NTSC-GROUP",
            "Inception",
            Some(2010),
            DvdR,
        ),
        ("Inception.2010.DVD-R-GROUP", "Inception", Some(2010), DvdR),
        (
            "Inception.2010.BRRip.XviD-GROUP",
            "Inception",
            Some(2010),
            Bluray480p,
        ),
        (
            "Inception.2010.BDRip.x264-GROUP",
            "Inception",
            Some(2010),
            Bluray720p,
        ),
        (
            "Inception.2010.576p.BluRay.x264-GROUP",
            "Inception",
            Some(2010),
            Bluray576p,
        ),
        (
            "Inception.2010.480p.BluRay.x264-GROUP",
            "Inception",
            Some(2010),
            Bluray480p,
        ),
        (
            "Inception.2010.COMPLETE.BLURAY-GROUP",
            "Inception",
            Some(2010),
            BrDisk,
        ),
        (
            "Inception.2010.COMPLETE.UHD.BLURAY-GROUP",
            "Inception",
            Some(2010),
            BrDisk,
        ),
        (
            "Inception.2010.1080p.BluRay.AVC.BD50-GROUP",
            "Inception",
            Some(2010),
            BrDisk,
        ),
        (
            "Inception.2010.HDCAM.x264-GROUP",
            "Inception",
            Some(2010),
            Cam,
        ),
        (
            "Inception.2010.CAM.XviD-GROUP",
            "Inception",
            Some(2010),
            Cam,
        ),
        (
            "Inception.2010.TS.XviD-GROUP",
            "Inception",
            Some(2010),
            Telesync,
        ),
        (
            "Inception.2010.HDTS.x264-GROUP",
            "Inception",
            Some(2010),
            Telesync,
        ),
        (
            "Inception.2010.TELESYNC.x264-GROUP",
            "Inception",
            Some(2010),
            Telesync,
        ),
        (
            "Inception.2010.TC.XviD-GROUP",
            "Inception",
            Some(2010),
            Telecine,
        ),
        (
            "Inception.2010.DVDSCR.XviD-GROUP",
            "Inception",
            Some(2010),
            DvdScr,
        ),
        (
            "Inception.2010.SCREENER.x264-GROUP",
            "Inception",
            Some(2010),
            DvdScr,
        ),
        (
            "Inception.2010.R5.XviD-GROUP",
            "Inception",
            Some(2010),
            Regional,
        ),
        (
            "Inception.2010.WORKPRINT.XviD-GROUP",
            "Inception",
            Some(2010),
            WorkPrint,
        ),
        (
            "Inception.2010.HDTV.720p.x264-GROUP",
            "Inception",
            Some(2010),
            Hdtv720p,
        ),
        (
            "Inception.2010.1080p.HDTV.x264-GROUP",
            "Inception",
            Some(2010),
            Hdtv1080p,
        ),
        (
            "Inception.2010.HDRip.x264-GROUP",
            "Inception",
            Some(2010),
            WebRip480p,
        ),
        (
            "Inception.2010.1080p.x264-GROUP",
            "Inception",
            Some(2010),
            Hdtv1080p,
        ),
        ("Inception.2010.XviD-GROUP", "Inception", Some(2010), Sdtv),
        ("Inception.2010-GROUP", "Inception", Some(2010), Unknown),
        (
            "2001.A.Space.Odyssey.1968.1080p.BluRay.x264-GROUP",
            "2001 A Space Odyssey",
            Some(1968),
            Bluray1080p,
        ),
        (
            "1917.2019.1080p.BluRay.x264-GROUP",
            "1917",
            Some(2019),
            Bluray1080p,
        ),
        (
            "Blade.Runner.2049.2017.2160p.UHD.BluRay.x265-GROUP",
            "Blade Runner 2049",
            Some(2017),
            Bluray2160p,
        ),
        (
            "Blade.Runner.2049.1080p.WEB-DL.x264-GROUP",
            "Blade Runner 2049",
            None,
            WebDl1080p,
        ),
        (
            "The.French.Dispatch.2021.1080p.WEB-DL.DDP5.1.H.264-GROUP",
            "The French Dispatch",
            Some(2021),
            WebDl1080p,
        ),
        (
            "Alien.1979.Directors.Cut.1080p.BluRay.x264-GROUP",
            "Alien",
            Some(1979),
            Bluray1080p,
        ),
        (
            "Aliens.1986.Special.Edition.REMASTERED.720p.BluRay.x264-GROUP",
            "Aliens",
            Some(1986),
            Bluray720p,
        ),
        (
            "Spider-Man.No.Way.Home.2021.1080p.WEB-DL.DDP5.1.H.264-GROUP",
            "Spider-Man No Way Home",
            Some(2021),
            WebDl1080p,
        ),
        (
            "Mission.Impossible.Dead.Reckoning.Part.One.2023.2160p.WEB-DL.DDP5.1.Atmos.DV.HDR.H.265-GROUP",
            "Mission Impossible Dead Reckoning Part One",
            Some(2023),
            WebDl2160p,
        ),
        (
            "Star Wars Episode IV A New Hope (1977) 1080p BluRay x264",
            "Star Wars Episode IV A New Hope",
            Some(1977),
            Bluray1080p,
        ),
        (
            "Amelie (2001) [1080p] [BluRay] [5.1] [YTS.MX]",
            "Amelie",
            Some(2001),
            Bluray1080p,
        ),
        (
            "Parasite (2019) [720p] [BluRay] [YTS.MX]",
            "Parasite",
            Some(2019),
            Bluray720p,
        ),
        (
            "Spirited Away (2001) [2160p] [4K] [WEB] [5.1] [YTS.MX]",
            "Spirited Away",
            Some(2001),
            WebDl2160p,
        ),
        (
            "Dune Part Two 2024 1080p WEBRip x265 10bit AAC5.1-GROUP",
            "Dune Part Two",
            Some(2024),
            WebRip1080p,
        ),
        (
            "Dune.Part.Two.2024.MULTi.1080p.BluRay.x264-GROUP",
            "Dune Part Two",
            Some(2024),
            Bluray1080p,
        ),
        (
            "Dune.Part.Two.2024.FRENCH.1080p.WEB.H264-GROUP",
            "Dune Part Two",
            Some(2024),
            WebDl1080p,
        ),
        (
            "Dune.Part.Two.2024.GERMAN.DL.1080p.BluRay.x264-GROUP",
            "Dune Part Two",
            Some(2024),
            Bluray1080p,
        ),
        (
            "Dune.Part.Two.2024.PROPER.1080p.WEB.H264-GROUP",
            "Dune Part Two",
            Some(2024),
            WebDl1080p,
        ),
        (
            "Dune.Part.Two.2024.REPACK.720p.BluRay.x264-GROUP",
            "Dune Part Two",
            Some(2024),
            Bluray720p,
        ),
        (
            "Dune.Part.Two.2024.HC.1080p.WEBRip.x264-GROUP",
            "Dune Part Two",
            Some(2024),
            WebRip1080p,
        ),
        (
            "Dune.Part.Two.2024.KORSUB.720p.HDRip.x264-GROUP",
            "Dune Part Two",
            Some(2024),
            WebRip720p,
        ),
        (
            "Oppenheimer.2023.IMAX.2160p.BluRay.x265-GROUP",
            "Oppenheimer",
            Some(2023),
            Bluray2160p,
        ),
        (
            "Oppenheimer.2023.1080p.BluRay.x264.DTS-HD.MA.5.1-GROUP.mkv",
            "Oppenheimer",
            Some(2023),
            Bluray1080p,
        ),
        (
            "Oppenheimer.2023.1080p.WEB.H264-GROUP[rarbg]",
            "Oppenheimer",
            Some(2023),
            WebDl1080p,
        ),
        (
            "Oppenheimer 2023 1080p WEB-DL DDP5.1 Atmos H.264-GROUP (TGx)",
            "Oppenheimer",
            Some(2023),
            WebDl1080p,
        ),
        (
            "Oppenheimer.(2023).1080p.WEB-DL",
            "Oppenheimer",
            Some(2023),
            WebDl1080p,
        ),
        (
            "Oppenheimer_2023_1080p_BluRay_x264-GROUP",
            "Oppenheimer",
            Some(2023),
            Bluray1080p,
        ),
        (
            "Godzilla.Minus.One.2023.JAPANESE.1080p.WEB-DL.DDP5.1.H.264-GROUP",
            "Godzilla Minus One",
            Some(2023),
            WebDl1080p,
        ),
        (
            "Godzilla.Minus.One.2023.1080p.WEB.x265-GROUP",
            "Godzilla Minus One",
            Some(2023),
            WebDl1080p,
        ),
        (
            "Godzilla Minus One 2023 1080p 10bit WEBRip 6CH x265 HEVC-GROUP",
            "Godzilla Minus One",
            Some(2023),
            WebRip1080p,
        ),
        (
            "Godzilla.Minus.One.2023.UHD.BluRay.2160p.TrueHD.Atmos.7.1.DV.HEVC.REMUX-GROUP",
            "Godzilla Minus One",
            Some(2023),
            Remux2160p,
        ),
        (
            "Nosferatu.1922.720p.BluRay.FLAC.1.0.x264-GROUP",
            "Nosferatu",
            Some(1922),
            Bluray720p,
        ),
        (
            "Metropolis.1927.RESTORED.1080p.BluRay.x264-GROUP",
            "Metropolis",
            Some(1927),
            Bluray1080p,
        ),
        (
            "Casablanca.1942.1080p.BluRay.REMUX.AVC.FLAC.1.0-GROUP",
            "Casablanca",
            Some(1942),
            Remux1080p,
        ),
        (
            "Mad.Max.Fury.Road.2015.1080p.BluRay.DDP5.1.x265.10bit-GROUP",
            "Mad Max Fury Road",
            Some(2015),
            Bluray1080p,
        ),
        (
            "Mad Max: Fury Road (2015) 1080p",
            "Mad Max: Fury Road",
            Some(2015),
            Hdtv1080p,
        ),
        (
            "Interstellar.2014.2160p.UHD.BluRay.X265.10bit.HDR.TrueHD.7.1.Atmos-GROUP",
            "Interstellar",
            Some(2014),
            Bluray2160p,
        ),
        (
            "Interstellar.2014.4K.HDR.2160p.BDRip.Ita.Eng.x265-GROUP",
            "Interstellar",
            Some(2014),
            Bluray2160p,
        ),
        (
            "Interstellar.2014.iTALiAN.MD.720p.BluRay.x264-GROUP",
            "Interstellar",
            Some(2014),
            Bluray720p,
        ),
        (
            "Interstellar 2014 Hindi Dubbed 720p WEBRip",
            "Interstellar",
            Some(2014),
            WebRip720p,
        ),
        (
            "[Group] Your Name (2016) [BD 1080p HEVC FLAC]",
            "Your Name",
            Some(2016),
            Bluray1080p,
        ),
        (
            "The.Movie.2020.1080p.WEB-DL.DDP5.1.H.264-GROUP",
            "The Movie",
            Some(2020),
            WebDl1080p,
        ),
        (
            "Movie.Title.2020.Extended.Cut.1080p.BluRay.x264-GROUP",
            "Movie Title",
            Some(2020),
            Bluray1080p,
        ),
        (
            "Movie.Title.2020.UNRATED.720p.WEB-DL.x264-GROUP",
            "Movie Title",
            Some(2020),
            WebDl720p,
        ),
        (
            "Movie.Title.2020.LiMiTED.DVDRip.x264-GROUP",
            "Movie Title",
            Some(2020),
            Dvd,
        ),
        (
            "Movie.Title.2020.INTERNAL.1080p.BluRay.x264-GROUP",
            "Movie Title",
            Some(2020),
            Bluray1080p,
        ),
        (
            "Movie.Title.1080p.BluRay.x264-GROUP",
            "Movie Title",
            None,
            Bluray1080p,
        ),
        (
            "Movie.Title.PROPER.DVDRip.XviD-GROUP",
            "Movie Title",
            None,
            Dvd,
        ),
        (
            "Movie Title 2020 Raw-HD 1080i MPEG2-GROUP",
            "Movie Title",
            Some(2020),
            RawHd,
        ),
        (
            "Movie.Title.2020.BDMV-GROUP",
            "Movie Title",
            Some(2020),
            BrDisk,
        ),
        (
            "Movie.Title.2020.1080p.AV1.WEBRip-GROUP",
            "Movie Title",
            Some(2020),
            WebRip1080p,
        ),
        (
            "Movie.Title.2020.1920x1080.WEB-DL-GROUP",
            "Movie Title",
            Some(2020),
            WebDl1080p,
        ),
        (
            "Movie.Title.2020.DSNP.WEB-DL.1080p.DDP5.1.H264-GROUP",
            "Movie Title",
            Some(2020),
            WebDl1080p,
        ),
        (
            "Movie.Title.2020.ATVP.WEB-DL.2160p.DV.HDR.DDP5.1.Atmos.H.265-GROUP",
            "Movie Title",
            Some(2020),
            WebDl2160p,
        ),
        (
            "Movie.Title.2020.iTunes.1080p.WEB-DL-GROUP",
            "Movie Title",
            Some(2020),
            WebDl1080p,
        ),
    ];
    let mut fails = Vec::new();
    for (name, title, year, q) in cases {
        let m = parse_movie(name);
        if m.title != *title || m.year != *year || m.quality.quality != *q {
            fails.push(format!(
                "{name}\n   got {:?} {:?} {:?}\n  want {title:?} {year:?} {q:?}",
                m.title, m.year, m.quality.quality
            ));
        }
    }
    report(fails, cases.len());
}

#[test]
fn episodes() {
    let cases: &[(&str, &str, Option<u32>, &[u32], Quality)] = &[
        (
            "The.Office.US.S05E10.720p.HDTV.x264-GROUP",
            "The Office US",
            Some(5),
            &[10],
            Hdtv720p,
        ),
        (
            "The.Office.US.S05E10.HDTV.XviD-GROUP",
            "The Office US",
            Some(5),
            &[10],
            Sdtv,
        ),
        (
            "The.Office.US.S05E10.1080p.WEB-DL.DD5.1.H.264-GROUP",
            "The Office US",
            Some(5),
            &[10],
            WebDl1080p,
        ),
        (
            "The.Office.US.S05E10.1080p.BluRay.x264-GROUP",
            "The Office US",
            Some(5),
            &[10],
            Bluray1080p,
        ),
        (
            "The.Office.US.S05E10.E11.720p.HDTV.x264-GROUP",
            "The Office US",
            Some(5),
            &[10, 11],
            Hdtv720p,
        ),
        (
            "The.Office.US.S05E10E11.720p.HDTV.x264-GROUP",
            "The Office US",
            Some(5),
            &[10, 11],
            Hdtv720p,
        ),
        (
            "The.Office.US.S05E10-E12.720p.HDTV.x264-GROUP",
            "The Office US",
            Some(5),
            &[10, 11, 12],
            Hdtv720p,
        ),
        (
            "The.Office.US.S05E10-12.720p.HDTV.x264-GROUP",
            "The Office US",
            Some(5),
            &[10, 11, 12],
            Hdtv720p,
        ),
        (
            "The.Office.US.S05E10E11E12.720p.HDTV.x264-GROUP",
            "The Office US",
            Some(5),
            &[10, 11, 12],
            Hdtv720p,
        ),
        (
            "The Office (US) - S05E10 - Episode Title [HDTV-720p]",
            "The Office (US)",
            Some(5),
            &[10],
            Hdtv720p,
        ),
        (
            "The Office (US) - 5x10 - Episode Title",
            "The Office (US)",
            Some(5),
            &[10],
            Unknown,
        ),
        (
            "the.office.us.s05e10.720p.hdtv.x264-group",
            "the office us",
            Some(5),
            &[10],
            Hdtv720p,
        ),
        (
            "Breaking.Bad.S01E01.Pilot.1080p.BluRay.x264-GROUP",
            "Breaking Bad",
            Some(1),
            &[1],
            Bluray1080p,
        ),
        (
            "Breaking Bad S01E01 Pilot 1080p BluRay x264-GROUP",
            "Breaking Bad",
            Some(1),
            &[1],
            Bluray1080p,
        ),
        (
            "Breaking_Bad_S01E01_720p_HDTV_x264-GROUP",
            "Breaking Bad",
            Some(1),
            &[1],
            Hdtv720p,
        ),
        (
            "Breaking.Bad.1x01.720p.HDTV.x264-GROUP",
            "Breaking Bad",
            Some(1),
            &[1],
            Hdtv720p,
        ),
        (
            "Breaking.Bad.1x01-1x02.720p.HDTV.x264-GROUP",
            "Breaking Bad",
            Some(1),
            &[1, 2],
            Hdtv720p,
        ),
        (
            "Breaking.Bad.101.HDTV.XviD-GROUP",
            "Breaking Bad",
            Some(1),
            &[1],
            Sdtv,
        ),
        (
            "Breaking.Bad.s01.e01.720p.hdtv-GROUP",
            "Breaking Bad",
            Some(1),
            &[1],
            Hdtv720p,
        ),
        (
            "Breaking.Bad.S01E01.2160p.WEB.H265-GROUP",
            "Breaking Bad",
            Some(1),
            &[1],
            WebDl2160p,
        ),
        (
            "Game.of.Thrones.S08E06.The.Iron.Throne.1080p.AMZN.WEB-DL.DDP5.1.H.264-GROUP",
            "Game of Thrones",
            Some(8),
            &[6],
            WebDl1080p,
        ),
        (
            "Game.of.Thrones.S08E06.2160p.UHD.BluRay.REMUX.HDR.HEVC.Atmos-GROUP",
            "Game of Thrones",
            Some(8),
            &[6],
            Remux2160p,
        ),
        (
            "Game.of.Thrones.S08E06.720p.WEBRip.x264-GROUP",
            "Game of Thrones",
            Some(8),
            &[6],
            WebRip720p,
        ),
        (
            "Game.of.Thrones.S08E06.PROPER.720p.HDTV.x264-GROUP",
            "Game of Thrones",
            Some(8),
            &[6],
            Hdtv720p,
        ),
        (
            "Game.of.Thrones.S08E06.REPACK.1080p.WEB.H264-GROUP",
            "Game of Thrones",
            Some(8),
            &[6],
            WebDl1080p,
        ),
        (
            "Game of Thrones S08E06 1080p WEB H264-GROUP[eztv]",
            "Game of Thrones",
            Some(8),
            &[6],
            WebDl1080p,
        ),
        (
            "Doctor.Who.2005.S13E01.1080p.HDTV.x264-GROUP",
            "Doctor Who",
            Some(13),
            &[1],
            Hdtv1080p,
        ),
        (
            "Doctor Who (2005) S13E01 1080p",
            "Doctor Who",
            Some(13),
            &[1],
            Hdtv1080p,
        ),
        (
            "House.of.the.Dragon.S02E01.1080p.WEB.H264-GROUP",
            "House of the Dragon",
            Some(2),
            &[1],
            WebDl1080p,
        ),
        (
            "Marvels.Agents.of.S.H.I.E.L.D.S01E01.720p.HDTV.X264-GROUP",
            "Marvels Agents of S.H.I.E.L.D",
            Some(1),
            &[1],
            Hdtv720p,
        ),
        (
            "9-1-1.S05E10.720p.HDTV.x264-GROUP",
            "9-1-1",
            Some(5),
            &[10],
            Hdtv720p,
        ),
        (
            "24.S08E01.720p.HDTV.x264-GROUP",
            "24",
            Some(8),
            &[1],
            Hdtv720p,
        ),
        (
            "The.100.S07E16.1080p.WEB.H264-GROUP",
            "The 100",
            Some(7),
            &[16],
            WebDl1080p,
        ),
        (
            "Mr.Robot.S04E13.1080p.AMZN.WEB-DL.DDP5.1.H.264-GROUP",
            "Mr Robot",
            Some(4),
            &[13],
            WebDl1080p,
        ),
        (
            "Shogun.2024.S01E01.Anjin.2160p.DSNP.WEB-DL.DDP5.1.HDR.H.265-GROUP",
            "Shogun",
            Some(1),
            &[1],
            WebDl2160p,
        ),
        (
            "Shogun 2024 S01E01 1080p HEVC x265-GROUP",
            "Shogun",
            Some(1),
            &[1],
            Hdtv1080p,
        ),
        (
            "The.Bear.S03E01.Tomorrow.1080p.HULU.WEB-DL.DDP5.1.H.264-GROUP",
            "The Bear",
            Some(3),
            &[1],
            WebDl1080p,
        ),
        (
            "Severance.S02E10.Cold.Harbor.2160p.ATVP.WEB-DL.DDP5.1.Atmos.DV.H.265-GROUP",
            "Severance",
            Some(2),
            &[10],
            WebDl2160p,
        ),
        (
            "Severance.S02E10.720p.WEB.x265-GROUP",
            "Severance",
            Some(2),
            &[10],
            WebDl720p,
        ),
        (
            "The.Last.of.Us.S01E03.Long.Long.Time.1080p.HMAX.WEB-DL.DDP5.1.Atmos.H.264-GROUP",
            "The Last of Us",
            Some(1),
            &[3],
            WebDl1080p,
        ),
        (
            "Dark.S01E01.GERMAN.DL.1080p.WEBRip.x264-GROUP",
            "Dark",
            Some(1),
            &[1],
            WebRip1080p,
        ),
        (
            "Dark.S01E01.MULTi.1080p.NF.WEB-DL.DDP5.1.x264-GROUP",
            "Dark",
            Some(1),
            &[1],
            WebDl1080p,
        ),
        (
            "Lupin.S01E01.FRENCH.720p.WEB.H264-GROUP",
            "Lupin",
            Some(1),
            &[1],
            WebDl720p,
        ),
        (
            "Squid.Game.S01E01.KOREAN.1080p.NF.WEBRip.DDP5.1.x264-GROUP",
            "Squid Game",
            Some(1),
            &[1],
            WebRip1080p,
        ),
        (
            "Money.Heist.S01E01.SPANISH.480p.WEB-DL.x264-GROUP",
            "Money Heist",
            Some(1),
            &[1],
            WebDl480p,
        ),
        (
            "Sherlock.S04E00.The.Abominable.Bride.1080p.BluRay.x264-GROUP",
            "Sherlock",
            Some(4),
            &[0],
            Bluray1080p,
        ),
        (
            "Show.Name.S00E01.Special.720p.HDTV.x264-GROUP",
            "Show Name",
            Some(0),
            &[1],
            Hdtv720p,
        ),
        (
            "Show.Name.S01E01.DVDRip.XviD-GROUP",
            "Show Name",
            Some(1),
            &[1],
            Dvd,
        ),
        (
            "Show.Name.S01E01.PDTV.XviD-GROUP",
            "Show Name",
            Some(1),
            &[1],
            Sdtv,
        ),
        (
            "Show.Name.S01E01.DSR.XviD-GROUP",
            "Show Name",
            Some(1),
            &[1],
            Sdtv,
        ),
        (
            "Show.Name.S01E01.576p.BluRay.x264-GROUP",
            "Show Name",
            Some(1),
            &[1],
            Bluray576p,
        ),
        (
            "Show.Name.S01E01.HDTV.1080i.MPEG2-GROUP",
            "Show Name",
            Some(1),
            &[1],
            RawHd,
        ),
        (
            "Show.Name.S01E01.1080p.WEB-DL.AAC2.0.H.264-GROUP.mkv",
            "Show Name",
            Some(1),
            &[1],
            WebDl1080p,
        ),
        (
            "Show.Name.S01E01.1080p.iT.WEB-DL.DD5.1.H.264-GROUP",
            "Show Name",
            Some(1),
            &[1],
            WebDl1080p,
        ),
        (
            "Show.Name.S01E01.1080p.PCOK.WEB-DL.DDP5.1.H.264-GROUP",
            "Show Name",
            Some(1),
            &[1],
            WebDl1080p,
        ),
        (
            "Show.Name.S01E01.1080p.AHDTV.x264-GROUP",
            "Show Name",
            Some(1),
            &[1],
            Hdtv1080p,
        ),
        (
            "Show.Name.S01E01.720p.HDTV.x264-GROUP",
            "Show Name",
            Some(1),
            &[1],
            Hdtv720p,
        ),
        (
            "Show.Name.S01E100.720p.HDTV.x264-GROUP",
            "Show Name",
            Some(1),
            &[100],
            Hdtv720p,
        ),
        (
            "Show.Name.S2010E05.720p.HDTV.x264-GROUP",
            "Show Name",
            Some(2010),
            &[5],
            Hdtv720p,
        ),
        (
            "Show.Name.S01.E01.720p.HDTV.x264-GROUP",
            "Show Name",
            Some(1),
            &[1],
            Hdtv720p,
        ),
        (
            "Show.Name.S01E01-720p.HDTV.x264-GROUP",
            "Show Name",
            Some(1),
            &[1],
            Hdtv720p,
        ),
        (
            "Show.Name.S01E01.INTERNAL.720p.WEB.h264-GROUP",
            "Show Name",
            Some(1),
            &[1],
            WebDl720p,
        ),
        (
            "Show Name - S01E01 - Title - WEBDL-1080p",
            "Show Name",
            Some(1),
            &[1],
            WebDl1080p,
        ),
        (
            "Show Name - S01E01E02 - Title [Bluray-1080p]",
            "Show Name",
            Some(1),
            &[1, 2],
            Bluray1080p,
        ),
        (
            "Show.Name.US.S01E01.720p.HDTV.x264-GROUP",
            "Show Name US",
            Some(1),
            &[1],
            Hdtv720p,
        ),
        (
            "Show.Name.UK.S01E01.720p.HDTV.x264-GROUP",
            "Show Name UK",
            Some(1),
            &[1],
            Hdtv720p,
        ),
        (
            "Show.Name.2019.S01E01.720p.HDTV.x264-GROUP",
            "Show Name",
            Some(1),
            &[1],
            Hdtv720p,
        ),
        (
            "Show.Name.(2019).S01E01.720p.HDTV.x264-GROUP",
            "Show Name",
            Some(1),
            &[1],
            Hdtv720p,
        ),
        (
            "Show.Name.S01E01.Episode.Name.With.1080p.In.It.720p.HDTV.x264-GROUP",
            "Show Name",
            Some(1),
            &[1],
            Hdtv1080p,
        ),
        (
            "Show.Name.S03E07.1080p.HEVC.x265-GROUP",
            "Show Name",
            Some(3),
            &[7],
            Hdtv1080p,
        ),
        (
            "Show.Name.S03E07.XviD-GROUP",
            "Show Name",
            Some(3),
            &[7],
            Sdtv,
        ),
        (
            "Show.Name.S03E07.HC.HDRip.x264-GROUP",
            "Show Name",
            Some(3),
            &[7],
            WebRip480p,
        ),
    ];
    let mut fails = Vec::new();
    for (name, series, season, eps, q) in cases {
        match parse_episode(name) {
            Some(e)
                if e.series == *series
                    && e.season == *season
                    && e.episodes == *eps
                    && e.quality.quality == *q => {}
            other => fails.push(format!(
                "{name}\n   got {:?}\n  want {series:?} {season:?} {eps:?} {q:?}",
                other.map(|e| (e.series, e.season, e.episodes, e.quality.quality))
            )),
        }
    }
    report(fails, cases.len());
}

#[test]
fn season_packs() {
    // name, series, seasons, full
    let cases: &[(&str, &str, &[u32])] = &[
        (
            "The.Office.US.S05.720p.HDTV.x264-GROUP",
            "The Office US",
            &[5],
        ),
        (
            "The.Office.US.S05.COMPLETE.720p.WEB-DL.x264-GROUP",
            "The Office US",
            &[5],
        ),
        (
            "The Office US Season 5 Complete 720p",
            "The Office US",
            &[5],
        ),
        (
            "The.Office.US.Season.05.1080p.BluRay.x264-GROUP",
            "The Office US",
            &[5],
        ),
        (
            "The Office (US) S05 1080p BluRay x265-GROUP",
            "The Office (US)",
            &[5],
        ),
        (
            "The.Office.US.S01-S09.COMPLETE.1080p.WEB-DL-GROUP",
            "The Office US",
            &[1, 2, 3, 4, 5, 6, 7, 8, 9],
        ),
        (
            "The.Office.US.S01-09.1080p.BluRay.x264-GROUP",
            "The Office US",
            &[1, 2, 3, 4, 5, 6, 7, 8, 9],
        ),
        ("The Office US S01 - S03 720p", "The Office US", &[1, 2, 3]),
        (
            "The Office US Seasons 1-3 720p",
            "The Office US",
            &[1, 2, 3],
        ),
        (
            "The Office US Season 1-3 1080p",
            "The Office US",
            &[1, 2, 3],
        ),
        (
            "Breaking.Bad.S02.1080p.BluRay.x264-GROUP",
            "Breaking Bad",
            &[2],
        ),
        (
            "Breaking Bad (2008) Season 2 S02 (1080p BluRay x265 HEVC 10bit AAC 5.1)",
            "Breaking Bad",
            &[2],
        ),
        (
            "Dark.S03.GERMAN.DL.1080p.NF.WEB-DL.DDP5.1.x264-GROUP",
            "Dark",
            &[3],
        ),
        (
            "Severance.S01.2160p.ATVP.WEB-DL.DDP5.1.DV.H.265-GROUP",
            "Severance",
            &[1],
        ),
        ("Show.Name.S10.720p.HDTV.x264-GROUP", "Show Name", &[10]),
        ("Show_Name_S01_1080p_WEB-DL", "Show Name", &[1]),
    ];
    let mut fails = Vec::new();
    for (name, series, seasons) in cases {
        match parse_episode(name) {
            Some(e)
                if e.series == *series
                    && e.seasons == *seasons
                    && e.full_season
                    && e.episodes.is_empty()
                    && e.season == seasons.first().copied() => {}
            other => fails.push(format!(
                "{name}\n   got {other:?}\n  want {series:?} {seasons:?}"
            )),
        }
    }
    for name in [
        "Show.Name.The.Complete.Series.1080p.BluRay.x264-GROUP",
        "Show Name Complete Series 720p",
    ] {
        match parse_episode(name) {
            Some(e) if e.series == "Show Name" && e.full_season && e.season.is_none() => {}
            other => fails.push(format!("{name}\n   got {other:?}")),
        }
    }
    report(fails, cases.len() + 2);
}

#[test]
fn daily_shows() {
    let cases: &[(&str, &str, (u16, u8, u8))] = &[
        (
            "The.Daily.Show.2024.05.18.Guest.Name.720p.WEB.h264-GROUP",
            "The Daily Show",
            (2024, 5, 18),
        ),
        (
            "The Daily Show 2024 05 18 720p WEB h264-GROUP",
            "The Daily Show",
            (2024, 5, 18),
        ),
        (
            "The.Daily.Show.2024-05-18.720p.WEB.h264-GROUP",
            "The Daily Show",
            (2024, 5, 18),
        ),
        (
            "Conan.2010.11.08.Guest.HDTV.XviD-GROUP",
            "Conan",
            (2010, 11, 8),
        ),
        (
            "Late.Night.Show.2023.12.31.1080p.WEB.h264-GROUP",
            "Late Night Show",
            (2023, 12, 31),
        ),
        (
            "Late.Night.Show.12.31.2023.1080p.WEB.h264-GROUP",
            "Late Night Show",
            (2023, 12, 31),
        ),
        (
            "Late.Night.Show.31.12.2023.1080p.WEB.h264-GROUP",
            "Late Night Show",
            (2023, 12, 31),
        ),
        (
            "News.Hour.2022.01.03.720p.HDTV.x264-GROUP",
            "News Hour",
            (2022, 1, 3),
        ),
        (
            "WWE.Raw.2024.01.01.1080p.HDTV.h264-GROUP",
            "WWE Raw",
            (2024, 1, 1),
        ),
        (
            "Morning_Show_2019_06_15_HDTV",
            "Morning Show",
            (2019, 6, 15),
        ),
    ];
    let mut fails = Vec::new();
    for (name, series, date) in cases {
        match parse_episode(name) {
            Some(e) if e.series == *series && e.air_date == Some(*date) && e.season.is_none() => {}
            other => fails.push(format!("{name}\n   got {other:?}")),
        }
    }
    report(fails, cases.len());
}

#[test]
fn anime() {
    // name, series, season, episodes (seasonal) or absolute, quality, group
    let cases: &[(&str, &str, Option<u32>, &[u32], &[u32], Quality, &str)] = &[
        (
            "[SubsPlease] Jujutsu Kaisen - 24 (1080p) [ABCD1234].mkv",
            "Jujutsu Kaisen",
            None,
            &[],
            &[24],
            Hdtv1080p,
            "SubsPlease",
        ),
        (
            "[SubsPlease] Jujutsu Kaisen - 24 (720p) [ABCD1234].mkv",
            "Jujutsu Kaisen",
            None,
            &[],
            &[24],
            Hdtv720p,
            "SubsPlease",
        ),
        (
            "[Erai-raws] One Piece - 1085 [1080p][Multiple Subtitle][ABCDEF12].mkv",
            "One Piece",
            None,
            &[],
            &[1085],
            Hdtv1080p,
            "Erai-raws",
        ),
        (
            "[Erai-raws] One Piece - 1085v2 [1080p].mkv",
            "One Piece",
            None,
            &[],
            &[1085],
            Hdtv1080p,
            "Erai-raws",
        ),
        (
            "[HorribleSubs] Boku no Hero Academia - 88 [720p].mkv",
            "Boku no Hero Academia",
            None,
            &[],
            &[88],
            Hdtv720p,
            "HorribleSubs",
        ),
        (
            "[Group] Frieren - 01-28 (BD 1080p) [Batch]",
            "Frieren",
            None,
            &[],
            &(1..=28).collect::<Vec<_>>().leak()[..],
            Bluray1080p,
            "Group",
        ),
        (
            "[Group] Frieren S2 - 05 [1080p]",
            "Frieren",
            Some(2),
            &[5],
            &[],
            Hdtv1080p,
            "Group",
        ),
        (
            "[Group] Frieren - S01E05 [WEB 1080p]",
            "Frieren",
            Some(1),
            &[5],
            &[],
            WebDl1080p,
            "Group",
        ),
        (
            "[Group] Spy x Family - 12 [WEB 1080p AV1][Multi-Subs]",
            "Spy x Family",
            None,
            &[],
            &[12],
            WebDl1080p,
            "Group",
        ),
        (
            "[Group] Mob Psycho 100 - 03 [1080p]",
            "Mob Psycho 100",
            None,
            &[],
            &[3],
            Hdtv1080p,
            "Group",
        ),
        (
            "[Group] Steins;Gate - 01 [BD 720p]",
            "Steins;Gate",
            None,
            &[],
            &[1],
            Bluray720p,
            "Group",
        ),
        (
            "[Group] Cowboy Bebop - 05 [DVD 480p]",
            "Cowboy Bebop",
            None,
            &[],
            &[5],
            Dvd,
            "Group",
        ),
        (
            "[Group]_Naruto_-_220_[480p].mkv",
            "Naruto",
            None,
            &[],
            &[220],
            Sdtv,
            "Group",
        ),
        (
            "Naruto Shippuden - 500 [1080p]",
            "Naruto Shippuden",
            None,
            &[],
            &[500],
            Hdtv1080p,
            "",
        ),
        (
            "[Group] Attack on Titan - 87 [1080p HEVC]",
            "Attack on Titan",
            None,
            &[],
            &[87],
            Hdtv1080p,
            "Group",
        ),
        (
            "[Group] Bleach - 366 (1080p WEB-DL)",
            "Bleach",
            None,
            &[],
            &[366],
            WebDl1080p,
            "Group",
        ),
    ];
    let mut fails = Vec::new();
    for (name, series, season, eps, abs, q, group) in cases {
        let want_group = (!group.is_empty()).then(|| group.to_string());
        match parse_episode(name) {
            Some(e)
                if e.series == *series
                    && e.season == *season
                    && e.episodes == *eps
                    && e.absolute == *abs
                    && e.quality.quality == *q
                    && (group.is_empty() || e.quality.group == want_group) => {}
            other => fails.push(format!(
                "{name}\n   got {:?}",
                other.map(|e| (
                    e.series,
                    e.season,
                    e.episodes,
                    e.absolute,
                    e.quality.quality,
                    e.quality.group
                ))
            )),
        }
    }
    report(fails, cases.len());
}

#[test]
fn specials_and_non_episodes() {
    let e = parse_episode("Sherlock.S04E00.The.Abominable.Bride.1080p.BluRay.x264-GROUP").unwrap();
    assert!(e.special);
    let e = parse_episode("Show.Name.S02.Special.720p.HDTV.x264-GROUP").unwrap();
    assert!(e.special && e.season == Some(2));
    let e = parse_episode("[Group] Title - 05 OVA [1080p]").unwrap();
    assert!(e.special);
    assert!(
        !parse_episode("Show.Name.S01E01.720p.HDTV.x264-GROUP")
            .unwrap()
            .special
    );
    for movie in [
        "Inception.2010.1080p.BluRay.x264-GROUP",
        "The Matrix (1999) 2160p UHD BluRay",
        "Amelie (2001) [1080p] [BluRay]",
    ] {
        assert!(parse_episode(movie).is_none(), "{movie}");
    }
}

#[test]
fn quality_details() {
    struct C {
        n: &'static str,
        codec: Option<&'static str>,
        hdr: &'static [&'static str],
        audio: Option<&'static str>,
        ch: Option<&'static str>,
        atmos: bool,
        group: Option<&'static str>,
        version: u8,
        langs: &'static [&'static str],
        hc: bool,
    }
    const fn c(n: &'static str) -> C {
        C {
            n,
            codec: None,
            hdr: &[],
            audio: None,
            ch: None,
            atmos: false,
            group: None,
            version: 1,
            langs: &[],
            hc: false,
        }
    }
    let cases = [
        C {
            codec: Some("H.264"),
            audio: Some("DD+"),
            ch: Some("5.1"),
            group: Some("NTb"),
            ..c("Show.S01E01.1080p.AMZN.WEB-DL.DDP5.1.H.264-NTb")
        },
        C {
            codec: Some("H.265"),
            hdr: &["DV", "HDR"],
            audio: Some("DD+"),
            ch: Some("5.1"),
            atmos: true,
            group: Some("FLUX"),
            ..c("Movie.2021.2160p.WEB-DL.DDP5.1.Atmos.DV.HDR.H.265-FLUX")
        },
        C {
            codec: Some("H.265"),
            hdr: &["HDR10+"],
            audio: Some("TrueHD"),
            ch: Some("7.1"),
            atmos: true,
            group: Some("GRP"),
            ..c("Movie.2021.2160p.UHD.BluRay.x265.HDR10+.TrueHD.7.1.Atmos-GRP")
        },
        C {
            codec: Some("H.265"),
            hdr: &["HDR10"],
            audio: Some("DTS-HD MA"),
            ch: Some("5.1"),
            group: Some("GRP"),
            ..c("Movie.2021.2160p.BluRay.HEVC.HDR10.DTS-HD.MA.5.1-GRP")
        },
        C {
            codec: Some("H.264"),
            audio: Some("DTS"),
            group: Some("GRP"),
            ..c("Movie.2021.1080p.BluRay.DTS.x264-GRP")
        },
        C {
            codec: Some("H.264"),
            audio: Some("DTS:X"),
            ch: None,
            group: Some("GRP"),
            ..c("Movie.2021.1080p.BluRay.DTS-X.x264-GRP")
        },
        C {
            codec: Some("H.264"),
            audio: Some("AAC"),
            ch: Some("2.0"),
            group: Some("GRP"),
            ..c("Show.S01E01.720p.WEB-DL.AAC2.0.H.264-GRP")
        },
        C {
            codec: Some("H.264"),
            audio: Some("DD"),
            ch: Some("5.1"),
            group: Some("GRP"),
            ..c("Show.S01E01.720p.WEB-DL.DD5.1.H.264-GRP")
        },
        C {
            codec: Some("H.264"),
            audio: Some("DD"),
            group: Some("GRP"),
            ..c("Movie.2010.DVDRip.AC3.x264-GRP")
        },
        C {
            codec: Some("XviD"),
            audio: Some("MP3"),
            group: Some("GRP"),
            ..c("Movie.2010.DVDRip.XviD.MP3-GRP")
        },
        C {
            codec: Some("AV1"),
            audio: Some("Opus"),
            group: Some("GRP"),
            ..c("Movie.2010.1080p.WEBRip.AV1.Opus-GRP")
        },
        C {
            codec: Some("VC-1"),
            group: Some("GRP"),
            ..c("Movie.2010.1080p.BluRay.VC-1.REMUX-GRP")
        },
        C {
            codec: Some("MPEG-2"),
            group: Some("GRP"),
            ..c("Movie.2010.DVD9.MPEG-2-GRP")
        },
        C {
            codec: Some("H.265"),
            hdr: &["HLG"],
            group: Some("GRP"),
            ..c("Show.S01E01.2160p.HLG.HEVC-GRP")
        },
        C {
            codec: Some("H.265"),
            hdr: &["DV"],
            group: Some("GRP"),
            ..c("Movie.2023.2160p.DoVi.x265-GRP")
        },
        C {
            codec: Some("H.264"),
            version: 2,
            group: Some("GRP"),
            ..c("Movie.2010.PROPER.1080p.BluRay.x264-GRP")
        },
        C {
            codec: Some("H.264"),
            version: 2,
            group: Some("GRP"),
            ..c("Movie.2010.REPACK.1080p.BluRay.x264-GRP")
        },
        C {
            codec: Some("H.264"),
            version: 3,
            group: Some("GRP"),
            ..c("Movie.2010.REPACK2.1080p.BluRay.x264-GRP")
        },
        C {
            codec: Some("H.264"),
            version: 2,
            group: Some("GRP"),
            ..c("Movie.2010.RERIP.1080p.BluRay.x264-GRP")
        },
        C {
            version: 2,
            group: Some("Group"),
            ..c("[Group] Title - 05v2 [1080p]")
        },
        C {
            version: 3,
            group: Some("Group"),
            ..c("[Group] Title - 05 [v3][1080p]")
        },
        C {
            codec: Some("H.264"),
            langs: &["Multi"],
            group: Some("GRP"),
            ..c("Movie.2010.MULTi.1080p.BluRay.x264-GRP")
        },
        C {
            codec: Some("H.264"),
            langs: &["French"],
            group: Some("GRP"),
            ..c("Movie.2010.TRUEFRENCH.1080p.BluRay.x264-GRP")
        },
        C {
            codec: Some("H.264"),
            langs: &["French"],
            group: Some("GRP"),
            ..c("Movie.2010.VFF.1080p.BluRay.x264-GRP")
        },
        C {
            codec: Some("H.264"),
            langs: &["German"],
            group: Some("GRP"),
            ..c("Movie.2010.German.1080p.BluRay.x264-GRP")
        },
        C {
            codec: Some("H.264"),
            langs: &["Italian"],
            group: Some("GRP"),
            ..c("Movie.2010.iTALiAN.1080p.BluRay.x264-GRP")
        },
        C {
            codec: Some("H.264"),
            langs: &["Spanish"],
            group: Some("GRP"),
            ..c("Movie.2010.SPANISH.1080p.BluRay.x264-GRP")
        },
        C {
            codec: Some("H.264"),
            langs: &["Spanish"],
            group: Some("GRP"),
            ..c("Movie.2010.1080p.LATINO.WEB-DL.x264-GRP")
        },
        C {
            codec: Some("H.264"),
            langs: &["Russian"],
            group: Some("GRP"),
            ..c("Movie.2010.RUS.1080p.BluRay.x264-GRP")
        },
        C {
            codec: Some("H.264"),
            langs: &["Hindi"],
            group: Some("GRP"),
            ..c("Movie.2010.Hindi.1080p.WEB-DL.x264-GRP")
        },
        C {
            codec: Some("H.264"),
            langs: &["Multi", "English", "Hindi"],
            group: Some("GRP"),
            ..c("Movie.2010.DUAL.English.Hindi.1080p.WEB-DL.x264-GRP")
        },
        C {
            codec: Some("H.264"),
            langs: &["Nordic"],
            group: Some("GRP"),
            ..c("Movie.2010.NORDiC.1080p.BluRay.x264-GRP")
        },
        C {
            codec: Some("H.264"),
            langs: &["Korean"],
            group: Some("GRP"),
            ..c("Movie.2010.KOREAN.1080p.BluRay.x264-GRP")
        },
        C {
            codec: Some("H.264"),
            langs: &["Polish"],
            group: Some("GRP"),
            ..c("Movie.2010.PLDUB.1080p.WEB-DL.x264-GRP")
        },
        C {
            codec: Some("H.264"),
            hc: true,
            group: Some("GRP"),
            ..c("Movie.2010.HC.1080p.WEBRip.x264-GRP")
        },
        C {
            codec: Some("H.264"),
            hc: true,
            group: Some("GRP"),
            ..c("Movie.2010.KORSUB.1080p.WEBRip.x264-GRP")
        },
        C {
            codec: Some("H.264"),
            hc: true,
            group: Some("GRP"),
            ..c("Movie.2010.1080p.WEBRip.x264.Subbed-GRP")
        },
        C {
            codec: Some("H.264"),
            group: Some("GRP"),
            ..c("Movie.2010.1080p.WEBRip.x264.SoftSubs-GRP")
        },
        C {
            group: None,
            ..c("Movie.2010.1080p.WEB-DL")
        },
        C {
            codec: Some("H.264"),
            group: None,
            ..c("Movie.2010.1080p.BluRay.x264")
        },
        C {
            codec: Some("H.264"),
            audio: Some("DTS-HD MA"),
            group: None,
            ..c("Movie.2010.1080p.BluRay.x264.DTS-HD")
        },
        C {
            codec: Some("H.264"),
            group: Some("GRP"),
            ..c("Movie.2010.1080p.BluRay.x264-GRP.mkv")
        },
        C {
            codec: Some("H.264"),
            group: Some("GRP"),
            ..c("Movie.2010.1080p.BluRay.x264-GRP[rarbg]")
        },
        C {
            codec: Some("H.264"),
            group: Some("GRP"),
            ..c("Movie 2010 1080p BluRay x264-GRP (TGx)")
        },
        C {
            codec: Some("H.264"),
            group: Some("GRP"),
            ..c("Movie_2010_1080p_BluRay_x264-GRP")
        },
    ];
    let mut fails = Vec::new();
    for w in &cases {
        let q = parse_quality(w.n);
        let langs: Vec<&str> = q.languages.clone();
        if q.codec != w.codec
            || q.hdr != w.hdr
            || q.audio != w.audio
            || q.channels.as_deref() != w.ch
            || q.atmos != w.atmos
            || q.group.as_deref() != w.group
            || q.version != w.version
            || langs != w.langs
            || q.hardcoded_subs != w.hc
        {
            fails.push(format!(
                "{}\n   got codec {:?} hdr {:?} audio {:?} ch {:?} atmos {} group {:?} v{} langs {:?} hc {}",
                w.n, q.codec, q.hdr, q.audio, q.channels, q.atmos, q.group, q.version, langs, q.hardcoded_subs
            ));
        }
    }
    report(fails, cases.len());
}

#[test]
fn music() {
    let cases: &[(&str, &str, &str, Option<u16>, Quality)] = &[
        (
            "Pink Floyd - The Dark Side of the Moon (1973) [FLAC]",
            "Pink Floyd",
            "The Dark Side of the Moon",
            Some(1973),
            Flac,
        ),
        (
            "Pink Floyd - The Dark Side of the Moon (1973) [FLAC 24bit 96kHz]",
            "Pink Floyd",
            "The Dark Side of the Moon",
            Some(1973),
            Flac24,
        ),
        (
            "Pink Floyd - The Dark Side of the Moon (2011 Remaster) [FLAC 24-96]",
            "Pink Floyd",
            "The Dark Side of the Moon",
            Some(2011),
            Flac24,
        ),
        (
            "Pink Floyd - The Dark Side of the Moon [24-bit Hi-Res FLAC]",
            "Pink Floyd",
            "The Dark Side of the Moon",
            None,
            Flac24,
        ),
        (
            "Pink Floyd - The Dark Side of the Moon (1973) [MP3 320]",
            "Pink Floyd",
            "The Dark Side of the Moon",
            Some(1973),
            Mp3_320,
        ),
        (
            "Pink Floyd - The Dark Side of the Moon (1973) [320kbps]",
            "Pink Floyd",
            "The Dark Side of the Moon",
            Some(1973),
            Mp3_320,
        ),
        (
            "Pink Floyd - The Dark Side of the Moon (1973) MP3 256kbps",
            "Pink Floyd",
            "The Dark Side of the Moon",
            Some(1973),
            Mp3_256,
        ),
        (
            "Pink Floyd - The Dark Side of the Moon (1973) [MP3 V0]",
            "Pink Floyd",
            "The Dark Side of the Moon",
            Some(1973),
            Mp3VbrV0,
        ),
        (
            "Pink Floyd - The Dark Side of the Moon (1973) [MP3 V2]",
            "Pink Floyd",
            "The Dark Side of the Moon",
            Some(1973),
            Mp3VbrV2,
        ),
        (
            "Pink Floyd - The Dark Side of the Moon (1973) [MP3 192]",
            "Pink Floyd",
            "The Dark Side of the Moon",
            Some(1973),
            Mp3_192,
        ),
        (
            "Pink Floyd - The Dark Side of the Moon (1973) [MP3 128kbps]",
            "Pink Floyd",
            "The Dark Side of the Moon",
            Some(1973),
            Mp3_128,
        ),
        (
            "Pink Floyd - The Dark Side of the Moon (1973) [MP3]",
            "Pink Floyd",
            "The Dark Side of the Moon",
            Some(1973),
            Mp3Unknown,
        ),
        (
            "Pink Floyd - The Dark Side of the Moon (1973) [AAC 256]",
            "Pink Floyd",
            "The Dark Side of the Moon",
            Some(1973),
            Aac,
        ),
        (
            "Pink Floyd - The Dark Side of the Moon (1973) [M4A]",
            "Pink Floyd",
            "The Dark Side of the Moon",
            Some(1973),
            Aac,
        ),
        (
            "Pink Floyd - The Dark Side of the Moon (1973) [ALAC]",
            "Pink Floyd",
            "The Dark Side of the Moon",
            Some(1973),
            Alac,
        ),
        (
            "Pink Floyd - The Dark Side of the Moon (1973) [APE]",
            "Pink Floyd",
            "The Dark Side of the Moon",
            Some(1973),
            Ape,
        ),
        (
            "Pink Floyd - The Dark Side of the Moon (1973) [WavPack]",
            "Pink Floyd",
            "The Dark Side of the Moon",
            Some(1973),
            WavPack,
        ),
        (
            "Pink Floyd - The Dark Side of the Moon (1973) [OGG]",
            "Pink Floyd",
            "The Dark Side of the Moon",
            Some(1973),
            Ogg,
        ),
        (
            "Pink Floyd - The Dark Side of the Moon (1973) [Lossless]",
            "Pink Floyd",
            "The Dark Side of the Moon",
            Some(1973),
            Flac,
        ),
        (
            "Pink Floyd - The Dark Side of the Moon (1973)",
            "Pink Floyd",
            "The Dark Side of the Moon",
            Some(1973),
            Unknown,
        ),
        (
            "Pink Floyd - 1973 - The Dark Side of the Moon [FLAC]",
            "Pink Floyd",
            "The Dark Side of the Moon",
            Some(1973),
            Flac,
        ),
        (
            "Pink Floyd - The Dark Side of the Moon {50th Anniversary} [FLAC]",
            "Pink Floyd",
            "The Dark Side of the Moon",
            None,
            Flac,
        ),
        (
            "Pink Floyd - The Dark Side of the Moon (Deluxe Edition) (2023) [FLAC]",
            "Pink Floyd",
            "The Dark Side of the Moon",
            Some(2023),
            Flac,
        ),
        (
            "Pink_Floyd-The_Dark_Side_Of_The_Moon-REMASTERED-CD-FLAC-2011-GROUP",
            "Pink Floyd",
            "The Dark Side Of The Moon",
            Some(2011),
            Flac,
        ),
        (
            "Pink_Floyd-The_Dark_Side_Of_The_Moon-WEB-2011-GROUP",
            "Pink Floyd",
            "The Dark Side Of The Moon",
            Some(2011),
            Unknown,
        ),
        (
            "Pink_Floyd-The_Dark_Side_Of_The_Moon-(CAT001)-24BIT-WEB-FLAC-2023-GROUP",
            "Pink Floyd",
            "The Dark Side Of The Moon",
            Some(2023),
            Flac24,
        ),
        (
            "Pink_Floyd-The_Dark_Side_Of_The_Moon-320-WEB-2011-GROUP",
            "Pink Floyd",
            "The Dark Side Of The Moon",
            Some(2011),
            Mp3_320,
        ),
        (
            "Pink_Floyd-The_Dark_Side_Of_The_Moon-V0-WEB-2011-GROUP",
            "Pink Floyd",
            "The Dark Side Of The Moon",
            Some(2011),
            Mp3VbrV0,
        ),
        (
            "Daft Punk - Random Access Memories (2013) [FLAC 24bit]",
            "Daft Punk",
            "Random Access Memories",
            Some(2013),
            Flac24,
        ),
        (
            "Daft Punk - Discovery (2001) [MP3 320]",
            "Daft Punk",
            "Discovery",
            Some(2001),
            Mp3_320,
        ),
        (
            "Daft_Punk-Discovery-2001-GROUP",
            "Daft Punk",
            "Discovery",
            Some(2001),
            Unknown,
        ),
        (
            "Miles Davis - Kind of Blue (1959) [FLAC]",
            "Miles Davis",
            "Kind of Blue",
            Some(1959),
            Flac,
        ),
        (
            "Miles Davis - Kind of Blue (1959) [Vinyl Rip 24-192 FLAC]",
            "Miles Davis",
            "Kind of Blue",
            Some(1959),
            Flac24,
        ),
        (
            "The Beatles - Abbey Road (1969) [MP3 320kbps]",
            "The Beatles",
            "Abbey Road",
            Some(1969),
            Mp3_320,
        ),
        (
            "The Beatles - Abbey Road (2019 Super Deluxe) [FLAC]",
            "The Beatles",
            "Abbey Road",
            Some(2019),
            Flac,
        ),
        (
            "Radiohead - OK Computer (1997) FLAC",
            "Radiohead",
            "OK Computer",
            Some(1997),
            Flac,
        ),
        (
            "Radiohead - OK Computer OKNOTOK 1997 2017 [FLAC]",
            "Radiohead",
            "OK Computer OKNOTOK",
            Some(1997),
            Flac,
        ),
        (
            "Various Artists - Now 100 (2018) [MP3 320]",
            "Various Artists",
            "Now 100",
            Some(2018),
            Mp3_320,
        ),
        (
            "VA-Summer_Hits-WEB-2020-GROUP",
            "VA",
            "Summer Hits",
            Some(2020),
            Unknown,
        ),
        (
            "Kendrick Lamar - DAMN. (2017) [WAV]",
            "Kendrick Lamar",
            "DAMN.",
            Some(2017),
            Wav,
        ),
    ];
    let mut fails = Vec::new();
    for (name, artist, album, year, q) in cases {
        let m = parse_music(name);
        if m.artist != *artist || m.album != *album || m.year != *year || m.quality != *q {
            fails.push(format!(
                "{name}\n   got {:?} {:?} {:?} {:?}",
                m.artist, m.album, m.year, m.quality
            ));
        }
    }
    report(fails, cases.len());
    let m = parse_music("Pink_Floyd-The_Dark_Side_Of_The_Moon-REMASTERED-CD-FLAC-2011-GROUP");
    assert!(m.lossless && m.format == Some("FLAC") && m.group.as_deref() == Some("GROUP"));
    let m = parse_music("Daft Punk - Discovery (2001) [MP3 320]");
    assert!(!m.lossless && m.bitrate == Some(320));
}

#[test]
fn books() {
    let cases: &[(&str, &str, &str, Quality, bool)] = &[
        (
            "Brandon Sanderson - The Way of Kings (2010) [epub]",
            "Brandon Sanderson",
            "The Way of Kings",
            Epub,
            false,
        ),
        (
            "Brandon Sanderson - The Way of Kings (2010) [mobi]",
            "Brandon Sanderson",
            "The Way of Kings",
            Mobi,
            false,
        ),
        (
            "Brandon Sanderson - The Way of Kings (2010) [azw3]",
            "Brandon Sanderson",
            "The Way of Kings",
            Azw3,
            false,
        ),
        (
            "Brandon Sanderson - The Way of Kings (2010) [pdf]",
            "Brandon Sanderson",
            "The Way of Kings",
            Pdf,
            false,
        ),
        (
            "Brandon Sanderson - The Way of Kings.epub",
            "Brandon Sanderson",
            "The Way of Kings",
            Epub,
            false,
        ),
        (
            "Brandon Sanderson - The Way of Kings (Retail) (epub)",
            "Brandon Sanderson",
            "The Way of Kings",
            Epub,
            false,
        ),
        (
            "Brandon Sanderson - The Stormlight Archive 01 - The Way of Kings [epub]",
            "Brandon Sanderson",
            "The Way of Kings",
            Epub,
            false,
        ),
        (
            "Brandon Sanderson - The Way of Kings (Unabridged) [M4B]",
            "Brandon Sanderson",
            "The Way of Kings",
            AudioM4b,
            true,
        ),
        (
            "Brandon Sanderson - The Way of Kings (Unabridged) [MP3]",
            "Brandon Sanderson",
            "The Way of Kings",
            AudioMp3,
            true,
        ),
        (
            "Brandon Sanderson - The Way of Kings [Audiobook] [FLAC]",
            "Brandon Sanderson",
            "The Way of Kings",
            AudioFlac,
            true,
        ),
        (
            "Brandon Sanderson - The Way of Kings [Audiobook]",
            "Brandon Sanderson",
            "The Way of Kings",
            AudioMp3,
            true,
        ),
        (
            "Brandon Sanderson - The Way of Kings.m4b",
            "Brandon Sanderson",
            "The Way of Kings",
            AudioM4b,
            true,
        ),
        (
            "The Way of Kings by Brandon Sanderson [epub]",
            "Brandon Sanderson",
            "The Way of Kings",
            Epub,
            false,
        ),
        (
            "The Way of Kings by Brandon Sanderson (2010) EPUB",
            "Brandon Sanderson",
            "The Way of Kings",
            Epub,
            false,
        ),
        (
            "The Way of Kings by Brandon Sanderson [M4B]",
            "Brandon Sanderson",
            "The Way of Kings",
            AudioM4b,
            true,
        ),
        (
            "Brandon.Sanderson-The.Way.of.Kings-RETAIL.EPUB.eBook-GROUP",
            "Brandon Sanderson",
            "The Way of Kings",
            Epub,
            false,
        ),
        (
            "Brandon.Sanderson-The.Way.of.Kings-AUDIOBOOK-MP3-GROUP",
            "Brandon Sanderson",
            "The Way of Kings",
            AudioMp3,
            true,
        ),
        (
            "Brandon_Sanderson-The_Way_of_Kings-Unabridged-M4B-2010-GROUP",
            "Brandon Sanderson",
            "The Way of Kings",
            AudioM4b,
            true,
        ),
        (
            "Jane Austen - Pride and Prejudice [epub]",
            "Jane Austen",
            "Pride and Prejudice",
            Epub,
            false,
        ),
        (
            "Jane Austen - Pride and Prejudice (1813) [pdf]",
            "Jane Austen",
            "Pride and Prejudice",
            Pdf,
            false,
        ),
        (
            "Jane Austen - Pride and Prejudice (Penguin Classics) [mobi]",
            "Jane Austen",
            "Pride and Prejudice (Penguin Classics)",
            Mobi,
            false,
        ),
        (
            "Frank Herbert - Dune (1965) [EPUB]",
            "Frank Herbert",
            "Dune",
            Epub,
            false,
        ),
        (
            "Frank Herbert - Dune [Unabridged] [MP3 64kbps]",
            "Frank Herbert",
            "Dune",
            AudioMp3,
            true,
        ),
        (
            "Frank Herbert - Dune (Audiobook) [M4B]",
            "Frank Herbert",
            "Dune",
            AudioM4b,
            true,
        ),
        (
            "Frank Herbert - Dune 1965 epub",
            "Frank Herbert",
            "Dune",
            Epub,
            false,
        ),
        (
            "J.R.R. Tolkien - The Hobbit [epub]",
            "J.R.R. Tolkien",
            "The Hobbit",
            Epub,
            false,
        ),
        (
            "J. R. R. Tolkien - The Hobbit (1937) [AZW3]",
            "J. R. R. Tolkien",
            "The Hobbit",
            Azw3,
            false,
        ),
        (
            "Ursula K. Le Guin - A Wizard of Earthsea [epub]",
            "Ursula K. Le Guin",
            "A Wizard of Earthsea",
            Epub,
            false,
        ),
        (
            "Isaac Asimov - Foundation (1951) [Retail EPUB]",
            "Isaac Asimov",
            "Foundation",
            Epub,
            false,
        ),
        (
            "Isaac Asimov - Foundation (1951) [KFX]",
            "Isaac Asimov",
            "Foundation",
            Azw3,
            false,
        ),
        (
            "Isaac Asimov - Foundation",
            "Isaac Asimov",
            "Foundation",
            Unknown,
            false,
        ),
        (
            "Mary Shelley - Frankenstein [Narrated by A Reader] [MP3]",
            "Mary Shelley",
            "Frankenstein",
            AudioMp3,
            true,
        ),
        (
            "Herman Melville - Moby Dick [pdf]",
            "Herman Melville",
            "Moby Dick",
            Pdf,
            false,
        ),
        (
            "Leo Tolstoy - War and Peace (Unabridged) (2015) [FLAC]",
            "Leo Tolstoy",
            "War and Peace",
            AudioFlac,
            true,
        ),
    ];
    let mut fails = Vec::new();
    for (name, author, title, q, audio) in cases {
        let b = parse_book(name);
        if b.author != *author || b.title != *title || b.quality != *q || b.audiobook != *audio {
            fails.push(format!(
                "{name}\n   got {:?} {:?} {:?} audio {}",
                b.author, b.title, b.quality, b.audiobook
            ));
        }
    }
    report(fails, cases.len());
    assert_eq!(
        parse_book("Jane Austen - Pride and Prejudice (1813) [pdf]").year,
        Some(1813)
    );
    assert_eq!(
        parse_book("Brandon.Sanderson-The.Way.of.Kings-RETAIL.EPUB.eBook-GROUP")
            .group
            .as_deref(),
        Some("GROUP")
    );
}

#[test]
fn normalized_titles() {
    let cases = [
        ("The Office (US)", "officeus"),
        ("Marvel's Agents of S.H.I.E.L.D.", "marvelsagentsshield"),
        ("Law & Order", "laworder"),
        ("Law and Order", "laworder"),
        ("A Quiet Place", "quietplace"),
        ("Spider-Man: No Way Home", "spidermannowayhome"),
        ("Amélie", "amélie"),
    ];
    for (a, b) in cases {
        assert_eq!(normalize_title(a), b, "{a}");
    }
    assert_eq!(
        normalize_title(&parse_movie("Spider-Man.No.Way.Home.2021.1080p.WEB-DL-GRP").title),
        normalize_title("Spider-Man: No Way Home")
    );
}
