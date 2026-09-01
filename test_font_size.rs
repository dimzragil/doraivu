use ratatui_image::picker::Picker;
fn main() {
    let picker = Picker::halfblocks();
    let fs = picker.font_size();
    let (w, h): (u16, u16) = fs.into();
    println!("{} {}", w, h);
}
