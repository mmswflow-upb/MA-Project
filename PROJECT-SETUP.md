# DIY Laptop Cooling Pad - README
Follow these steps for setting up the project (this guide is for windows only)


## RP Pico W Firmware

1. Make sure you have installed the Rust [toolchain manager](https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe),
if everything went well you should get the version of the toolchain in the terminal after running:
```
rustup --version
```

2. Install the elf2uf2-rs tool to be able to flash the firmware to the RP Pico W, run this command in the terminal:
```
cargo install elf2uf2-rs
```

3. Once you have cloned this repo, navigate to the rp_pico_w_firmware folder by running in the terminal
```
cd path/to/project-mmswflow-upb/rp_pico_w_firmware
```

4. Build the MCU's firmware by running
```
cargo build --release --target thumbv6m-none-eabi
```

5. The KiCAD schematic provides all the necessary information on how to connect the hardware components.

6. Make sure that you have connected your Pico W to your PC via USB

7. Flash the program by running this command
```
elf2uf2-rs -ds .\target\thumbv6m-none-eabi\release\pico_firmware
```
8. Now you can use the cooling pad.

## Python App

1. You must have Python 3.12 installed (or newer)

2. Once you have installed Python, open a terminal and install the following libraries
```
pip install pillow
pip install ttkbootstrap
pip install keyboard
```

3. Navigate to the Python App folder by using the terminal and running
```
cd path/to/project-mmswflow-upb/"Python App"
```

4. Run the program with the command:
```
python3 coolingstation_client.py
```
5. Now you can use the app to control the cooling pad from the laptop.

## Usage

1. The cooling pad must be turned on by pressing on the push button adjacent to the orange LED

2. Decreasing or increasing the power of the fans is done by pressing the buttons adjacent to the green & red LEDs

3. In order to use the WIFI feature of the cooling pad, you must first turn on mobile hotspot from your PC
```
Network Name: PicoProjectWifi
Network Password: 12345678
Network Band: 2.4 Ghz
```
5. Turning on the WIFI feature is done by pressing once on the button adjacent to the blue LED, forcing the MCU to try to connect to the laptop's hotspot network

6. Wait until the LCD displays `Wifi: Ready`, then open the app on your PC, and click on connect, wait for a few seconds and done! You can now control the power of the fans from the laptop through WIFI.
