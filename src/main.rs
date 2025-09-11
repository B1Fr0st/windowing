use windowing::hide_window;

fn main() {
    let window = windowing::find_window_by_pid(4160).unwrap().unwrap();
    hide_window(window);
}
