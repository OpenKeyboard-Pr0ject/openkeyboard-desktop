use std::io::{BufRead, Write};

use crate::hid_traffic::CaptureMark;

pub const CAPTURE_INSTRUCTIONS: &str = "\
Recording USB traffic. Do not type text on the keyboard while recording; use the mouse in the vendor tool.
Before each change in the vendor tool, describe it here and press Enter, for example \"brightness 60 to 40\".
Make one change at a time and wait a few seconds after it. Press Enter on an empty line to stop.";

pub fn prompt_for_marks(
    input: &mut impl BufRead,
    output: &mut impl Write,
    elapsed_micros: impl Fn() -> u64,
) -> Vec<CaptureMark> {
    let mut marks = Vec::new();
    let _ = writeln!(output, "{CAPTURE_INSTRUCTIONS}\n");
    loop {
        let _ = write!(output, "Next change: ");
        let _ = output.flush();
        let mut line = String::new();
        let label = match input.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => line.trim().to_string(),
        };
        if label.is_empty() {
            break;
        }
        marks.push(CaptureMark { label, at_micros: elapsed_micros() });
        let _ = writeln!(output, "  Make that change now, then describe the next one.");
    }
    marks
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::io::Cursor;

    use super::*;

    #[test]
    fn records_each_described_change_with_its_time() {
        let mut input = Cursor::new("open lighting page\n  brightness 60 to 40 \n\nignored\n");
        let mut output = Vec::new();
        let clock = Cell::new(0);
        let elapsed = || {
            clock.set(clock.get() + 1_000_000);
            clock.get()
        };

        let marks = prompt_for_marks(&mut input, &mut output, elapsed);

        assert_eq!(
            marks,
            vec![
                CaptureMark { label: "open lighting page".to_string(), at_micros: 1_000_000 },
                CaptureMark { label: "brightness 60 to 40".to_string(), at_micros: 2_000_000 },
            ]
        );
        assert!(String::from_utf8(output).unwrap().starts_with("Recording USB traffic."));
    }

    #[test]
    fn stops_at_the_end_of_input() {
        let marks = prompt_for_marks(&mut Cursor::new("only change"), &mut Vec::new(), || 5);

        assert_eq!(marks, vec![CaptureMark { label: "only change".to_string(), at_micros: 5 }]);
    }
}
