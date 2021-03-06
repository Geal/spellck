#![crate_id="spellck"]
#![deny(missing_doc)]
#![feature(managed_boxes)]

//! Prints the misspelled words in the public documentation &
//! identifiers of a crate.

extern crate collections;
extern crate getopts;
extern crate syntax;
extern crate rustc;
use std::{io, os, str};
use collections::{HashSet, PriorityQueue};
use syntax::{ast, codemap};
use rustc::driver::{driver, session};

pub mod words;
mod visitor;

static DEFAULT_DICT: &'static str = "/usr/share/dict/words";
static LIBDIR: &'static str = "/usr/local/lib/rustlib/x86_64-unknown-linux-gnu/lib";

fn main() {
    let args = std::os::args();
    let opts = ~[getopts::optmulti("d", "dict",
                                  "dictionary file (a list of words, one per line)", "PATH"),
                 getopts::optflag("n", "no-def-dict", "don't use the default dictionary"),
                 getopts::optflag("h", "help", "show this help message")];

    let matches = getopts::getopts(args.tail(), opts).unwrap();
    if matches.opts_present([~"h", ~"help"]) {
        println!("{}", getopts::usage(args[0], opts));
        return;
    }

    let mut words = HashSet::new();

    if !matches.opts_present([~"n", ~"no-def-dict"]) {
        if !read_lines_into(&Path::new(DEFAULT_DICT), &mut words) {
            return
        }
    }
    for dict in matches.opt_strs("d").move_iter().chain(matches.opt_strs("dict").move_iter()) {
        if !read_lines_into(&Path::new(dict), &mut words) {
            return
        }
    }

    // one visitor; the internal list of misspelled words gets reset
    // for each file, since the spans could conflict.
    let mut any_mistakes = false;

    for name in matches.free.iter() {
        let (sess, krate) = get_ast(Path::new(name.as_slice()));
        let cm = sess.codemap();

        let mut visitor = visitor::SpellingVisitor::new(&words);
        visitor.check_crate(&krate);

        struct Sort<'a> {
            sp: codemap::Span,
            words: &'a HashSet<~str>
        }
        impl<'a> Eq for Sort<'a> {
            fn eq(&self, other: &Sort<'a>) -> bool {
                self.sp == other.sp
            }
        }
        impl<'a> Ord for Sort<'a> {
            fn lt(&self, other: &Sort<'a>) -> bool {
                self.sp.lo < other.sp.lo ||
                    (self.sp.lo == other.sp.lo && self.sp.hi < other.sp.hi)
            }
        }

        // extract the lines in order of the spans, so that e.g. files
        // are grouped together, and lines occur in increasing order.
        let pq: PriorityQueue<Sort> =
            visitor.misspellings.iter().map(|(k, v)| Sort { sp: *k, words: v }).collect();

        // run through the spans, printing the words that are
        // apparently misspelled
        for Sort {sp, words} in pq.to_sorted_vec().move_iter() {
            any_mistakes = true;

            let lines = cm.span_to_lines(sp);
            let sp_text = cm.span_to_str(sp);

            // [] required for connect :(
            let word_vec: Vec<&str> = words.iter().map(|s| s.as_slice()).collect();

            println!("{}: misspelled {len, plural, =1{word} other{words}}: {}",
                     sp_text,
                     word_vec.connect(", "),
                     len=words.len());

            // first line; no lines = no printing
            match lines.lines.as_slice() {
                [line_num, ..] => {
                    let line = lines.file.get_line(line_num as int);
                    println!("{}: {}", sp_text, line);
                }
                _ => {}
            }
        }
    }

    if any_mistakes {
        os::set_exit_status(1)
    }
}

/// Load each line of the file `p` into the given `Extendable` object.
fn read_lines_into<E: Extendable<~str>>
                  (p: &Path, e: &mut E) -> bool {
    match io::File::open(p) {
        Ok(mut r) => {
            let s = str::from_utf8_owned(r.read_to_end().unwrap())
                .expect(format!("{} is not UTF-8", p.display()));
            e.extend(s.lines().map(|ss| ss.to_owned()));
            true
        }
        Err(e) => {
            (write!(&mut io::stderr() as &mut Writer,
                    "Error reading {}: {}", p.display(), e.to_str())).unwrap();
            os::set_exit_status(10);
            false
        }
    }
}

/// Extract the expanded ast of a crate, along with the codemap which
/// connects source code locations to the actual code.
fn get_ast(path: Path) -> (session::Session, ast::Crate) {
    use syntax::diagnostic;

    // cargo culted from rustdoc_ng :(
    let input = driver::FileInput(path);

    let sessopts = session::Options {
        maybe_sysroot: Some(os::self_exe_path().unwrap().dir_path()),
        addl_lib_search_paths: std::cell::RefCell::new((~[Path::new(LIBDIR)]).move_iter().collect()),
        .. (session::basic_options()).clone()
    };

    let codemap = syntax::codemap::CodeMap::new();
    let diagnostic_handler = diagnostic::default_handler();
    let span_diagnostic_handler =
        diagnostic::mk_span_handler(diagnostic_handler, codemap);

    let sess = driver::build_session_(sessopts, None, span_diagnostic_handler);

    let cfg = driver::build_configuration(&sess);

    let krate = driver::phase_1_parse_input(&sess, cfg, &input);
    let krate = {
        let mut loader = rustc::metadata::creader::Loader::new(&sess);
        driver::phase_2_configure_and_expand(&sess, &mut loader, krate,
                                             &from_str("spellck").unwrap()).val0()
    };
    (sess, krate)
}
