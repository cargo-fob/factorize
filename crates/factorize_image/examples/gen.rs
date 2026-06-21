//! 데모용 테스트 PNG 생성기 — cargo run -p factorize_image --example gen
use image::{ImageBuffer, Rgb};

fn main() {
    let (w, h) = (3000u32, 3000u32);
    // 고주파 패턴이라 PNG 압축이 잘 안 됨 (데모용으로 큰 before 만들기)
    let img = ImageBuffer::from_fn(w, h, |x, y| {
        let r = ((x ^ y) & 0xFF) as u8;
        let g = ((x.wrapping_mul(3) ^ y.wrapping_mul(7)) & 0xFF) as u8;
        let b = ((x.wrapping_add(y).wrapping_mul(5)) & 0xFF) as u8;
        Rgb([r, g, b])
    });
    img.save("test.png").expect("save 실패");
    println!("test.png 생성 ({w}x{h})");
}
