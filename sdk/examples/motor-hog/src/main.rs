#![no_std]
#![no_main]

use cmf_wp2_app::Api;

cmf_wp2_app::entry!(main);

fn main(api: &'static Api) -> i32 {
    api.vibrate(1000, 31);
    loop {
        core::hint::spin_loop()
    }
}
