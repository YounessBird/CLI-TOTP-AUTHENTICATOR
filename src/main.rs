use anyhow::{Context, Result};
use byteorder::{BigEndian, ReadBytesExt};
use crossterm::{
    event::{self, Event as CEvent, KeyCode},
    terminal::{disable_raw_mode, enable_raw_mode},
};
use ring::hmac;
use std::error::Error;
use std::io;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::vec;
use tui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Span, Spans},
    widgets::{
        Block, BorderType, Borders, Cell, Gauge, List, ListItem, ListState, Paragraph, Row, Table,
        Tabs,
    },
    Terminal,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // tui Gui
    enable_raw_mode().expect("can run in raw mode");

    // channel to communicate between input and rendering loop we want a channel and a thread for a loop to not block the main thread
    // create multiproducer, single consumer channel
    let (tx, rx) = mpsc::channel();
    // the tick rate
    let tick_rate = Duration::from_millis(200);
    thread::spawn(move || {
        // start counting from now
        let mut last_tick = Instant::now();
        //input loop
        loop {
            // calculate the next tick by subtracting tick_rate from last tick elapsed if the value is positive that value will be the timeout before sending an event else set it to 0 which mean no timeout
            let timeout = tick_rate
                .checked_sub(last_tick.elapsed()) // Duration subtraction. Computes self - other, returning None if the result would be negative or if overflow occurred.
                .unwrap_or_else(|| Duration::from_secs(0));
            //use event::poll to wait until that time for an event and if there is one,
            //send that input event through our channel with the key the user pressed.
            if event::poll(timeout).expect("poll works") {
                // read the event key
                if let CEvent::Key(key) = event::read().expect("can read events") {
                    tx.send(Event::Input(key)).expect("can send events");
                }
            }
            // if last tick elapsed is greter than tick rate send a tick ans start again
            if last_tick.elapsed() >= tick_rate {
                if let Ok(_) = tx.send(Event::Tick) {
                    last_tick = Instant::now();
                }
            }
        }
    });
    // create a terminal from crossterm backend
    let stdout = io::stdout();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    //Menu titles
    let menu_titles = vec!["Home", "Codes", "Add", "Delete", "Quit"];
    // active Menu ->Home
    let mut active_menu_item = MenuItem::Home;
    let mut app = App::default();
    let mut key_input_flag = false;
    let mut active_menu_keys = true;
    //creare a list
    let mut code_list_state = ListState::default();
    code_list_state.select(Some(0));

    // loop to draw widgets into screen
    loop {
        // draw a rect / direc: vertical/margin 2
        terminal.draw(|rect| {
            let size = rect.size(); // this returns Terminal size

            let chunks_codes = Layout::default()
                .direction(Direction::Vertical)
                .margin(2)
                .constraints(
                    [
                        // Menu
                        //Content
                        //Footer
                        Constraint::Length(3), //three lines stay constant
                        Constraint::Min(1),    // the content will grow size min 2
                        Constraint::Length(3), // three lines stay constant
                    ]
                    .as_ref(),
                )
                .split(size);

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(2)
                .constraints(
                    [
                        Constraint::Length(3), //three lines stay constant
                        Constraint::Length(3), //three lines stay constant
                        Constraint::Length(3), //three lines stay constant
                        Constraint::Length(4),
                        Constraint::Length(3), // three lines stay constant
                    ]
                    .as_ref(),
                )
                .split(size);
            // prepare the footer
            let copyright = Paragraph::new("TOTP-CLI 2022 - Authenticator")
                .style(Style::default().fg(Color::LightCyan))
                .alignment(Alignment::Center)
                .block(
                    // put the copyright paragraph in this block
                    Block::default()
                        .borders(Borders::ALL)
                        .style(Style::default().fg(Color::White))
                        .title("TOTP")
                        .border_type(BorderType::Plain),
                );

            // create the Menu
            let menu = menu_titles
                .iter()
                .map(|t| {
                    let (first, rest) = t.split_at(1);
                    Spans::from(vec![
                        Span::styled(
                            first,
                            Style::default()
                                .fg(Color::Green)
                                .add_modifier(Modifier::UNDERLINED),
                        ),
                        Span::styled(rest, Style::default().fg(Color::White)),
                    ])
                })
                .collect();

            let tabs = Tabs::new(menu)
                .select(active_menu_item.into())
                .block(Block::default().title("Menu").borders(Borders::ALL))
                .style(Style::default().fg(Color::White))
                .highlight_style(Style::default().fg(Color::Yellow))
                .divider(Span::raw("|"));

            rect.render_widget(tabs, chunks_codes[0]);
            match active_menu_item {
                MenuItem::Home => rect.render_widget(render_home(), chunks_codes[1]),
                MenuItem::Codes => {
                    let codes_chunks = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints(
                            [
                                Constraint::Percentage(20),
                                Constraint::Percentage(40),
                                Constraint::Percentage(40),
                            ]
                            .as_ref(),
                        )
                        .split(chunks_codes[1]);
                    let bar_chunks = Layout::default()
                        .direction(Direction::Vertical)
                        .margin(4)
                        .constraints([Constraint::Percentage(10)].as_ref())
                        .split(codes_chunks[2]);
                    let (left, right) = render_code(&code_list_state, &app);
                    rect.render_stateful_widget(left, codes_chunks[0], &mut code_list_state);
                    rect.render_widget(right, codes_chunks[1]);
                    //progress bar
                    if app.keys.len() > 0 {
                        let gauge = Gauge::default()
                            .block(Block::default().title("30s Timer").borders(Borders::ALL))
                            .gauge_style(Style::default().fg(Color::Green))
                            .ratio(app.progress);
                        rect.render_widget(gauge, bar_chunks[0]);
                    }
                }
                MenuItem::AddCode => {
                    // input for gen code
                    let account = Paragraph::new(app.account.as_ref())
                        .style(match app.input_mode {
                            InputMode::Normal => Style::default(),
                            InputMode::Editing => Style::default().fg(Color::Yellow),
                        })
                        .block(Block::default().borders(Borders::ALL).title("address"));
                    rect.render_widget(account, chunks[1]);
                    // address
                    let keyinput = Paragraph::new(app.key.as_ref())
                        .style(match app.input_mode {
                            InputMode::Normal => Style::default(),
                            InputMode::Editing => Style::default().fg(Color::Yellow),
                        })
                        .block(Block::default().borders(Borders::ALL).title("secrectkey"));
                    rect.render_widget(keyinput, chunks[2]);

                    let instructions = Paragraph::new(vec![
                        Spans::from(vec![Span::raw("Press <Tab> To change Input")]),
                        Spans::from(vec![Span::raw("Press <Esc> to access the Menu")]),
                    ])
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .style(Style::default().fg(Color::LightCyan))
                            .title("Instructions")
                            .border_type(BorderType::Plain),
                    );
                    rect.render_widget(instructions, chunks[3]);
                }
            }

            rect.render_widget(copyright, chunks_codes[2]);
        })?;

        match rx.recv()? {
            Event::Input(event) => match event.code {
                KeyCode::Char('q') => {
                    if active_menu_keys {
                        disable_raw_mode()?;
                        terminal.show_cursor()?;
                        break;
                    } else {
                        if key_input_flag {
                            app.key.push('q');
                        } else {
                            app.account.push('q');
                        }
                    }
                }
                KeyCode::Char('h') => {
                    if active_menu_keys {
                        active_menu_item = MenuItem::Home
                    } else {
                        if key_input_flag {
                            app.key.push('h');
                        } else {
                            app.account.push('h');
                        }
                    }
                }
                KeyCode::Char('c') => {
                    if active_menu_keys {
                        active_menu_item = MenuItem::Codes
                    } else {
                        if key_input_flag {
                            app.key.push('c');
                        } else {
                            app.account.push('c');
                        }
                    }
                }
                KeyCode::Char('a') => {
                    if active_menu_keys {
                        active_menu_item = MenuItem::AddCode;
                        active_menu_keys = false;
                    } else {
                        if key_input_flag {
                            app.key.push('a');
                        } else {
                            app.account.push('a');
                        }
                    }
                }
                KeyCode::Char('d') => {
                    if active_menu_keys {
                        remove_code_at_index(&mut code_list_state, &mut app)
                            .expect("can remove pet");
                    } else {
                        if key_input_flag {
                            app.key.push('d');
                        } else {
                            app.account.push('d');
                        }
                    }
                }

                // KeyCode::Char('e') => {
                //     app.input_mode = InputMode::Editing;
                // }
                KeyCode::Char(c) => {
                    active_menu_keys = false;
                    if key_input_flag {
                        app.key.push(c);
                    } else {
                        app.account.push(c);
                    }
                }
                KeyCode::Esc => {
                    active_menu_keys = true;
                }

                KeyCode::Tab => {
                    if key_input_flag {
                        key_input_flag = false
                    } else {
                        key_input_flag = true
                    }
                }

                KeyCode::Enter => {
                    key_input_flag = false;

                    // call construct message function
                    let account: String = app.account.drain(..).collect();
                    let key: String = app.key.drain(..).collect();
                    if key.len() > 0 {
                        app.keys.push((key.clone(), account.clone(), 0))
                    } else {
                        //
                    }
                    let codemsg = code_constructor(key, account);
                    app.messages.push(codemsg.unwrap());
                }

                KeyCode::Backspace => {
                    if key_input_flag {
                        app.key.pop();
                    } else {
                        app.account.pop();
                    }
                }

                KeyCode::Down => {
                    if active_menu_keys {
                        if let Some(selected) = code_list_state.selected() {
                            let number_of_codes_gens = app.messages.len();
                            if selected >= number_of_codes_gens - 1 {
                                code_list_state.select(Some(0));
                            } else {
                                code_list_state.select(Some(selected + 1));
                            }
                        }
                    }
                }
                KeyCode::Up => {
                    if active_menu_keys {
                        if let Some(selected) = code_list_state.selected() {
                            let number_of_codes_gens = app.messages.len();
                            if selected > 0 {
                                code_list_state.select(Some(selected - 1));
                            } else {
                                code_list_state.select(Some(number_of_codes_gens - 1));
                            }
                        }
                    }
                }
                _ => {}
            },
            Event::Tick => {
                app.update();
            }
        }
    }

    Ok(())
}

// Home Layout
fn render_home<'a>() -> Paragraph<'a> {
    let home = Paragraph::new(vec![
        Spans::from(vec![Span::raw("")]),
        Spans::from(vec![Span::styled(
            "Time-based One-time Password (TOTP) Authenticator",
            Style::default().fg(Color::LightGreen),
        )]),
        Spans::from(vec![Span::raw("")]),
        Spans::from(vec![Span::raw("Press 'c' to access Codes")]),
        Spans::from(vec![Span::raw(
            "'a' to generate TOTP  and 'd' to delete the currently selected Code.",
        )]),
    ])
    .alignment(Alignment::Center)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .style(Style::default().fg(Color::White))
            .title("Home")
            .border_type(BorderType::Plain),
    );
    home
}

// LAYOUT FOR Codes tab
fn render_code<'a>(code_list_state: &ListState, app: &App) -> (List<'a>, Table<'a>) {
    // box for the accounts
    let accounts = Block::default()
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::White))
        .title("TOTS")
        .border_type(BorderType::Plain);
    // vecs totp
    let code_list = app.messages.clone();

    //list of accounts as ListItems
    let items: Vec<_> = code_list
        .iter()
        .map(|code| {
            ListItem::new(Spans::from(vec![Span::styled(
                code.address.clone(),
                Style::default(),
            )]))
        })
        .collect();

    //selected from list else default totp object
    let selected_code = match code_list.get(
        code_list_state
            .selected()
            .expect("there is always a selected code"),
    ) {
        Some(r) => r.clone(),
        _ => Totp::new(),
    };
    //make a list of accounts and place it in the box
    let list = List::new(items).block(accounts).highlight_style(
        Style::default()
            .bg(Color::Yellow)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD),
    );

    let code_detail = Table::new(vec![Row::new(vec![Cell::from(Span::raw(
        selected_code.key,
    ))])])
    .header(Row::new(vec![Cell::from(Span::styled(
        "Key",
        Style::default().add_modifier(Modifier::BOLD),
    ))]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .style(Style::default().fg(Color::White))
            .title("Detail")
            .border_type(BorderType::Plain),
    )
    .widths(&[Constraint::Min(1)]);
    (list, code_detail)
}

fn code_constructor(key: String, account: String) -> Result<Totp, Box<dyn Error>> {
    let totpcode = generate_code(key).unwrap();
    let code_gen = Totp {
        key: totpcode.to_string(),
        address: account,
    };
    Ok(code_gen)
}

fn remove_code_at_index(
    code_list_state: &mut ListState,
    app: &mut App,
) -> Result<(), Box<dyn Error>> {
    if let Some(selected) = code_list_state.selected() {
        app.messages.remove(selected);
        code_list_state.select(Some(if selected > 1 { selected - 1 } else { 0 }));
    }
    Ok(())
}

// generate TOTP code
fn generate_code(key: String) -> Result<u64, Box<dyn std::error::Error>> {
    let t0 = 0;
    let tx = 30;
    let start = SystemTime::now();
    let time_in_seconds = start
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs();

    //HOTP
    let ct = (time_in_seconds - t0) / tx;

    let ctk = key.as_bytes();

    let keyc = hmac::Key::new(hmac::HMAC_SHA256, &ctk);
    let s = hmac::sign(&keyc, &ct.to_be_bytes());
    let code;
    let mut signature = s.as_ref();

    if signature.len() < 32 {
        return generate_code(key);
    } else {
        code = signature
            .read_u64::<BigEndian>()
            .with_context(|| format!("could not parse integer"))?
            % (10_u64.pow(6));
    }

    Ok(code)
}

#[derive(Clone)]
struct Totp {
    key: String,
    address: String,
}
impl Totp {
    fn new() -> Totp {
        Totp {
            key: String::new(),
            address: String::new(),
        }
    }
}
impl PartialEq for Totp {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

enum Event<I> {
    Input(I),
    Tick,
}

#[derive(Copy, Clone, Debug)]
enum MenuItem {
    Home,
    Codes,
    AddCode,
}

impl From<MenuItem> for usize {
    fn from(input: MenuItem) -> usize {
        match input {
            MenuItem::Home => 0,
            MenuItem::Codes => 1,
            MenuItem::AddCode => 2,
        }
    }
}

enum InputMode {
    Normal,
    Editing,
}

/// App holds the state of the application
struct App {
    /// Current value of the input box
    account: String,
    key: String,
    /// Current input mode
    input_mode: InputMode,
    /// History of recorded messages
    messages: Vec<Totp>,
    progress: f64,
    keys: Vec<(String, String, u64)>,
}

impl App {
    fn update(&mut self) {
        for (k, a, _) in self.keys.iter() {
            let codemsg = code_constructor(k.to_string(), a.to_string()).unwrap();
            if !self.messages.contains(&(codemsg)) {
                match self.messages.iter_mut().find(|x| x.address == *a) {
                    Some(r) => {
                        r.key = codemsg.key;
                        self.progress = 0.0;
                    }
                    _ => (),
                }
            }
        }

        self.progress += 0.0065;

        if self.progress > 1.0 {
            self.progress = 0.0;
        }
    }
}

impl Default for App {
    fn default() -> App {
        App {
            account: String::new(),
            key: String::new(),
            input_mode: InputMode::Normal,
            messages: Vec::new(),
            progress: 0.0,
            keys: vec![],
        }
    }
}
