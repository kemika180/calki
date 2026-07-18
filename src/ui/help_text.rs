//! Static content for the F1 help modal — one `vec![Line]` arm per tab (0-8).
//! Extracted verbatim from `ui()`; all spans are built from `&'static str`
//! literals (or compile-time `env!`/`format!`), so the result is `'static`.

use ratatui::prelude::*;

/// Help-modal body lines for the given tab index; empty for out-of-range tabs.
pub(crate) fn help_content(tab: usize, palette: &crate::theme::Palette) -> Vec<Line<'static>> {
    match tab {
        0 => vec![
            Line::from(vec![Span::styled(
                "── Global & Panel Navigation ──",
                Style::default().bold().fg(palette.help_section),
            )]),
            Line::from(vec![
                Span::styled(
                    " F1                     ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Toggle this Help Guide modal",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " h / l                  ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Switch between Help Tabs (Left / Right)",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " 1 - 9                  ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Switch directly to Help Tabs 1 through 9",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " j / k                  ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Scroll Help Content (Down / Up)",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " F2 / F3                ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Toggle Wiki Map / Variables Panel",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " F4                     ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Toggle Editor Word Wrapping",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " /                      ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Search entire Wiki for keyword / notes",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " Shift-H / L            ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Move Focus Left / Right between active panels",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " Esc                    ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Escape modes / Return focus to Editor",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " Ctrl-q                 ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Exit the program (from any mode/panel)",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "── Editor & Wiki Note Operations ──",
                Style::default().bold().fg(palette.help_section),
            )]),
            Line::from(vec![
                Span::styled(
                    " Enter                  ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Follow [[Link]] (Normal) / Wrap selection in Link (Visual)",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " Backspace              ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Go back in note history (Normal mode)",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " t                      ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Toggle todo item checkbox [ ] <=> [x] / Convert plain list bullet to todo",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " Ctrl-d                 ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Delete current wiki note / file",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " Ctrl-s                 ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Save current note explicitly",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " Ctrl-e                 ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Open Export Menu (HTML / Markdown)",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "── Wiki Map Panel (focused) ──",
                Style::default().bold().fg(palette.help_section),
            )]),
            Line::from(vec![
                Span::styled(
                    " d / x / Del            ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled("Delete selected note file", Style::default().fg(palette.fg)),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "── Variables Panel (focused) ──",
                Style::default().bold().fg(palette.help_section),
            )]),
            Line::from(vec![
                Span::styled(
                    " y                      ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Yank/copy variable value to system clipboard",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " Enter / i              ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Insert variable name at editor cursor",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![Span::styled(
                " * Custom functions (e.g. f(x) = body) are also displayed in this sidebar.",
                Style::default().fg(palette.fg).italic(),
            )]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "── Configuration File (~/.config/calki/config.json) ──",
                Style::default().bold().fg(palette.help_section),
            )]),
            Line::from(vec![
                Span::styled(
                    " scrolloff                 ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled(
                    "Number of lines to keep visible above/below cursor (default: 5)",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " mouse_focus_on_hover      ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled(
                    "Switch panel focus by hovering mouse (default: true)",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " expand_variables_on_select",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled(
                    "Auto-expand variables sidebar when focused (default: false)",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " line_numbers              ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled(
                    "Line numbers display mode: 'None', 'Absolute', 'Relative' (default: 'None')",
                    Style::default().fg(palette.fg),
                ),
            ]),
        ],
        1 => vec![
            Line::from(vec![Span::styled(
                "── Basic Arithmetic & Functions ──",
                Style::default().bold().fg(palette.help_section),
            )]),
            Line::from(vec![
                Span::styled(
                    " abs(x)                 ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled("Absolute value of x", Style::default().fg(palette.fg)),
            ]),
            Line::from(vec![
                Span::styled(
                    " sqrt(x)                ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled(
                    "Square root of x (negative inputs return complex)",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " round(x, [n])          ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled(
                    "Round x to n decimal places (default 0)",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " ceil(x) / floor(x)     ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled("Ceiling / Floor function", Style::default().fg(palette.fg)),
            ]),
            Line::from(vec![
                Span::styled(
                    " mod(x, y)              ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled(
                    "Modulo remainder (also infix x % y)",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "── Exponentials & Logarithms ──",
                Style::default().bold().fg(palette.help_section),
            )]),
            Line::from(vec![
                Span::styled(
                    " exp(x)                 ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled("Exponential e^x", Style::default().fg(palette.fg)),
            ]),
            Line::from(vec![
                Span::styled(
                    " ln(x)                  ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled(
                    "Natural logarithm (negative real inputs return complex)",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " log(x)                 ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled("Base-10 logarithm", Style::default().fg(palette.fg)),
            ]),
            Line::from(vec![
                Span::styled(
                    " log(x, base)           ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled(
                    "Logarithm of x with arbitrary base (e.g. log(8, 2) => 3)",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " log2(x)                ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled("Base-2 logarithm", Style::default().fg(palette.fg)),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "── Trigonometry ──",
                Style::default().bold().fg(palette.help_section),
            )]),
            Line::from(vec![
                Span::styled(
                    " sin / cos / tan        ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled(
                    "Trigonometric sine, cosine, tangent (supports complex)",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " asin / acos / atan     ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled(
                    "Inverse arc sine, cosine, tangent",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " sinh / cosh / tanh     ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled(
                    "Hyperbolic sine, cosine, tangent",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " asinh / acosh / atanh  ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled(
                    "Inverse hyperbolic functions",
                    Style::default().fg(palette.fg),
                ),
            ]),
        ],
        2 => vec![
            Line::from(vec![Span::styled(
                "── Complex Numbers ──",
                Style::default().bold().fg(palette.help_section),
            )]),
            Line::from(vec![
                Span::styled(
                    " imaginary unit 'i'     ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled(
                    "Literal suffix (e.g. 3i, 2 + 5i)",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " Complex Arithmetic     ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled(
                    "Supports +, -, *, /, powers, and trig/log/sqrt/abs functions",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "── Symbolic Calculus & Solving ──",
                Style::default().bold().fg(palette.help_section),
            )]),
            Line::from(vec![
                Span::styled(
                    " diff(f, x) / der(f, x) ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled(
                    "Symbolic derivative of f with respect to variable x",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " solve(eq, x)           ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled(
                    "Solve linear equation eq for x (e.g. solve(2*x + 5 == 15, x) => 5)",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "── Radix Notation & Bitwise ──",
                Style::default().bold().fg(palette.help_section),
            )]),
            Line::from(vec![
                Span::styled(
                    " 0x... / 0b...          ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled(
                    "Hexadecimal / Binary integer literals",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " in hex / in bin        ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled(
                    "Convert and format output (e.g. 15 in hex => 0xF)",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " &  |  ~  <<  >>  xor   ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled(
                    "Bitwise AND, OR, NOT (~), Left/Right Shift, and XOR",
                    Style::default().fg(palette.fg),
                ),
            ]),
        ],
        3 => vec![
            Line::from(vec![Span::styled(
                "── Statistics & Aggregations ──",
                Style::default().bold().fg(palette.help_section),
            )]),
            Line::from(vec![
                Span::styled(
                    " sum(x, ...)            ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled(
                    "Sum of elements / arguments",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " prod(x, ...)           ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled(
                    "Product of list elements / arguments (combines units)",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " mean / average         ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled(
                    "Arithmetic mean of arguments",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " median(x, ...)         ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled("Median value of arguments", Style::default().fg(palette.fg)),
            ]),
            Line::from(vec![
                Span::styled(
                    " stddev / variance      ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled(
                    "Sample standard deviation / variance",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " count(x, ...)          ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled(
                    "Count the number of scalar items in lists/arguments",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "── Vectors, Matrices & Plotting ──",
                Style::default().bold().fg(palette.help_section),
            )]),
            Line::from(vec![
                Span::styled(
                    " len(list)              ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled("Length of list / vector", Style::default().fg(palette.fg)),
            ]),
            Line::from(vec![
                Span::styled(
                    " plot(list)             ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled(
                    "Draws Unicode sparkline trend (e.g. ▄▅▇█)",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " vdot / vadd / vsub     ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled(
                    "Vector dot product, addition, and subtraction",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " transpose / matmul     ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled(
                    "Matrix transpose and matrix multiplication",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " [[1,2],[3,4]] * B      ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled(
                    "Infix * multiplies matrices / scales by a scalar",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " det(A)                 ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled(
                    "Determinant of a square matrix",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " inv(A)                 ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled(
                    "Inverse of a square matrix (via Gauss-Jordan)",
                    Style::default().fg(palette.fg),
                ),
            ]),
        ],
        4 => vec![
            Line::from(vec![Span::styled(
                "── Predefined Constants ──",
                Style::default().bold().fg(palette.help_section),
            )]),
            Line::from(vec![
                Span::styled(
                    " pi                     ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled(
                    "3.1415926535... (Ratio of circle circumference to diameter)",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " e                      ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled(
                    "2.7182818284... (Euler's number)",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " c                      ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled(
                    "299,792,458 m/s (Speed of light constant)",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " g                      ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled(
                    "9.80665 m/s^2 (Standard acceleration of gravity)",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " G                      ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled(
                    "6.6743e-11 m^3/(kg*s^2) (Newtonian gravity constant)",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " h                      ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled(
                    "6.62607015e-34 kg*m^2/s (Planck constant)",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " hbar                   ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled(
                    "1.054571817e-34 kg*m^2/s (Reduced Planck constant)",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " kb                     ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled(
                    "1.380649e-23 kg*m^2/(s^2*K) (Boltzmann constant)",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " NA                     ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled(
                    "6.02214076e23 (Avogadro constant)",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " R                      ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled(
                    "8.314462618 kg*m^2/(s^2*K) (Molar gas constant)",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " me                     ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled(
                    "9.1093837015e-31 kg (Electron mass)",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " mp                     ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled(
                    "1.67262192369e-27 kg (Proton mass)",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " inf                    ",
                    Style::default().fg(palette.config_key).bold(),
                ),
                Span::styled(
                    "Infinity (mathematical constant)",
                    Style::default().fg(palette.fg),
                ),
            ]),
        ],
        5 => vec![
            Line::from(vec![Span::styled(
                "── Variables & Scoping ──",
                Style::default().bold().fg(palette.help_section),
            )]),
            Line::from(vec![
                Span::styled(
                    " x = value              ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Define a variable in global scope",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " { y = 2 }              ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Block scope: new local variables discard on exit",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "── Functions ──",
                Style::default().bold().fg(palette.help_section),
            )]),
            Line::from(vec![
                Span::styled(
                    " f(x) = x^2             ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Define user function f with parameter x",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " map(expr, L)           ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Evaluate expr for each element in list L",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " filter(e, L)           ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Filter list L keeping elements where boolean expr e is true",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " reduce(e, L)           ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Reduce list L using expr e (accumulator and element)",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " any(expr, L)           ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Returns true if expr is true for any element in list L",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " all(expr, L)           ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Returns true if expr is true for all elements in list L",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " zip(L1, L2)            ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Zip two lists L1 and L2 together into a list of pairs",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "── Control Flow & Loops ──",
                Style::default().bold().fg(palette.help_section),
            )]),
            Line::from(vec![
                Span::styled(
                    " if C A else B          ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "If condition C is true, evaluate A, else evaluate B",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " switch V {...}         ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Match value V against case patterns and default case",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " for x in L             ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Iterate through elements of list L using loop var x",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " while cond             ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Evaluate body repeatedly while cond is true",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " range(a,b,s)           ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Generate a list of numbers from a to b with step s",
                    Style::default().fg(palette.fg),
                ),
            ]),
        ],
        6 => vec![
            Line::from(vec![Span::styled(
                "── Implemented Markdown Formatting ──",
                Style::default().bold().fg(palette.help_section),
            )]),
            Line::from(vec![
                Span::styled(
                    " # Heading 1            ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Large header (supports # to ###### for H1 to H6)",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " **bold**               ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Bold text using double asterisks/underscores",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " *italic*               ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Italic text using single asterisks/underscores",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " ~~strike~~             ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Strikethrough text using double tildes",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " > quote                ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Blockquote format for emphasized paragraphs",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " ---                    ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Horizontal rule/divider (three hyphens or asterisks)",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " - list / 1.            ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Bullet lists (-/*) and ordered numbered lists",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " - [ ] todo             ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Interactive task list item (press 't' to toggle)",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " [[Link]]               ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Wiki-style link to navigate/create pages (press Enter)",
                    Style::default().fg(palette.fg),
                ),
            ]),
        ],
        7 => vec![
            Line::from(vec![Span::styled(
                "── Implemented Vim Motions & Editing ──",
                Style::default().bold().fg(palette.help_section),
            )]),
            Line::from(vec![
                Span::styled(
                    " h / j / k / l          ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Move cursor Left, Down, Up, Right",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " w / b / e              ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Move forward/backward word, or to word end",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " 0 / ^ / $              ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Move to line start, first non-blank, or line end",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " { / }                  ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Move cursor paragraph backward / forward",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " Ctrl-u / d             ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Scroll cursor half page Up / Down",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " i / a                  ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Enter Insert Mode before / after cursor",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " I / A                  ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Insert at first non-blank / Append at end of line",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " v                      ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Enter Visual Mode to select and edit text",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " x                      ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Delete character under cursor",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " r <char>               ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Replace character under cursor with <char>",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " ~                      ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Toggle character casing (Normal/Visual modes)",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " dd / dw / d$           ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Delete line, delete word, or delete to line end",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " cc / cw / C            ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Change (delete and insert) line, word, or to line end",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " u / Ctrl-r             ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Undo last change / Redo change",
                    Style::default().fg(palette.fg),
                ),
            ]),
        ],
        8 => vec![
            Line::from(vec![Span::styled(
                "── About calki ──",
                Style::default().bold().fg(palette.help_section),
            )]),
            Line::from(""),
            Line::from(vec![
                Span::styled(
                    " Author:                ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "Jessica Gurchiek (kemika / kemika180)",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " Version:               ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    format!("v{}", env!("CARGO_PKG_VERSION")),
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    " License:               ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled("GPL-3.0-only", Style::default().fg(palette.fg)),
            ]),
            Line::from(vec![
                Span::styled(
                    " Repository:            ",
                    Style::default().fg(palette.keybind_label).bold(),
                ),
                Span::styled(
                    "https://github.com/kemika180/calki",
                    Style::default().fg(palette.fg),
                ),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                " calki is a terminal-based Markdown note editor and interactive math sheet",
                Style::default().fg(palette.fg),
            )]),
            Line::from(vec![Span::styled(
                " calculator with local wiki-style link navigation, designed for fast and",
                Style::default().fg(palette.fg),
            )]),
            Line::from(vec![Span::styled(
                " efficient mathematical notebook keeping.",
                Style::default().fg(palette.fg),
            )]),
        ],
        _ => Vec::new(),
    }
}
