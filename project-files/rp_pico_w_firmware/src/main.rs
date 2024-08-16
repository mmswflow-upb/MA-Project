#![no_std]
#![no_main]

use core::str::{from_utf8, FromStr};

use cortex_m::prelude::_embedded_hal_blocking_delay_DelayMs;

use cyw43_pio::PioSpi;
use embassy_executor::Spawner;
use embassy_net::tcp::TcpSocket;
use embassy_net::{Config, Stack, StackResources};
use embassy_rp::bind_interrupts;
use embassy_rp::gpio::{Input, Level, Output, OutputOpenDrain, Pull};
use embassy_rp::peripherals::{DMA_CH0, PIO0};
use embassy_rp::pio::{InterruptHandler, Pio};
use embassy_time::{with_timeout, Delay, Duration, TimeoutError, Timer};
use embedded_io_async::Write;

use embassy_rp::i2c::{Config as I2cConfig, I2c, InterruptHandler as I2CInterruptHandler};
use embassy_rp::peripherals::I2C0;

use embassy_futures::select::Either3::{First as First_3, Second as Second_3, Third as Third_3};
use embassy_futures::select::Either4::{
    First as First_4, Fourth, Second as Second_4, Third as Third_4,
};
use embassy_futures::select::{select3, select4};
use static_cell::StaticCell;

use heapless::String;
use lcd1602_driver::command::{self, State};
use lcd1602_driver::lcd::{self, Basic, Ext};
use lcd1602_driver::sender;
use log::{info, warn};

use embassy_rp::pwm::{Config as PwmConfig, Pwm};
use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;
use embassy_sync::channel::{Channel as MPMC_Channel, Receiver, Sender};

// USB driver
use embassy_rp::peripherals::USB;
use embassy_rp::usb::{Driver, InterruptHandler as USBInterruptHandler};

use panic_probe as _;

//ENUMS for channels, we use these when we switch the circuit on or off, or when we want the connection task to resume or pause

enum PowerCommand {
    Increase,
    Decrease,
}

// STRUCTS

//Debouncer struct, used to debounce the buttons
pub struct Debouncer<'a> {
    input: Input<'a>,
    debounce: u16,
}

impl<'a> Debouncer<'a> {
    pub fn new(input: Input<'a>, debounce: u16) -> Self {
        Self { input, debounce }
    }

    pub async fn debounce(&mut self) -> Level {
        loop {
            let l1 = self.input.get_level();

            self.input.wait_for_any_edge().await;

            Delay::delay_ms(&mut Delay, self.debounce);

            let l2 = self.input.get_level();
            if l1 != l2 {
                break l2;
            }
        }
    }
}

//CONSTANTS

const TOP: u16 = 0x8000; //This is the top value for the PWM
const DISPLAY_FREQUENCY: u32 = 100_000; //This is the frequency of the display
const LCD_ADDR: u8 = 0x27; //This is the address of the LCD
const WIFI_NETWORK: &str = "PicoProjectWifi";
const WIFI_PASSWORD: &str = "12345678";
const DEBOUNCE: u16 = 100; //This is the debounce time for the buttons [in ms]
const BUTTONS_TASK_DELAY: u64 = 400; //This is the delay for the buttons tasks [in ms]
const SPEED_CHANGE_DELAY: Duration = Duration::from_millis(400); //This is the delay for the power change [in ms]

/*CHANNELS:
- SPEED_CONTROL_CHANNEL: MPMC Channel for sending power commands to the main task
- SEND_OVER_CONNECTION_CHANNEL: MPMC Channel for sending data to the exchange over connection task
- SETUP_SWITCH_CHANNEL: MPMC Channel for sending signals to main to stop  the laptop pad from doing anything until power is switched on again
    - WIFI_SWITCH_CHANNEL: MPMC Channel for sending signals to the main task to switch wifi on or off
    - WIFI_MAIN_SWITCH_CHANNEL: MPMC Channel for sending signals to the exchange over connection task to switch wifi on or off
    - WIFI_CONNECTION_BREAK_CHANNEL: MPMC Channel for sending signals to the main task to notify it that the connection has been broken
    - CONNECTION_READY_CHANNEL: MPMC Channel for sending signals to the exchange over connection task to notify it that the connection is ready to be used

*/
static SPEED_CONTROL_CHANNEL: MPMC_Channel<ThreadModeRawMutex, (PowerCommand, bool), 64> =
    MPMC_Channel::new();
static SEND_OVER_CONNECTION_CHANNEL: MPMC_Channel<ThreadModeRawMutex, u8, 64> = MPMC_Channel::new();
static SETUP_SWITCH_CHANNEL: MPMC_Channel<ThreadModeRawMutex, bool, 64> = MPMC_Channel::new();
static WIFI_BTN_SWITCH_CHANNEL: MPMC_Channel<ThreadModeRawMutex, bool, 64> = MPMC_Channel::new();
static WIFI_MAIN_SWITCH_CHANNEL: MPMC_Channel<ThreadModeRawMutex, bool, 64> = MPMC_Channel::new();
static WIFI_CONNECTION_BREAK_CHANNEL: MPMC_Channel<ThreadModeRawMutex, bool, 64> =
    MPMC_Channel::new();
static CONNECTION_READY_CHANNEL: MPMC_Channel<ThreadModeRawMutex, bool, 64> = MPMC_Channel::new();
bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => USBInterruptHandler<USB>;
    // PIO interrupt for CYW SPI communication
    PIO0_IRQ_0 => InterruptHandler<PIO0>;
    I2C0_IRQ => I2CInterruptHandler<I2C0>;
});

//useful functions
fn match_power(power: u8) -> String<32> {
    match power {
        80 => {
            return String::<32>::try_from("Power: Medium").unwrap();
        }
        100 => {
            return String::<32>::try_from("Power: High").unwrap();
        }
        _ => {
            return String::<32>::try_from("Power: Low").unwrap();
        }
    }
}

//UTILITY TASKS

#[embassy_executor::task]
async fn logger_task(driver: Driver<'static, USB>) {
    embassy_usb_logger::run!(1024, log::LevelFilter::Info, driver);
}

#[embassy_executor::task]
async fn wifi_task(
    runner: cyw43::Runner<'static, Output<'static>, PioSpi<'static, PIO0, 0, DMA_CH0>>,
) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn net_task(stack: &'static Stack<cyw43::NetDriver<'static>>) -> ! {
    stack.run().await
}

#[embassy_executor::task]
async fn exchange_over_connection(
    mut wifi_control: cyw43::Control<'static>,
    stack: &'static Stack<cyw43::NetDriver<'static>>,
    power_control_sender: Sender<'static, ThreadModeRawMutex, (PowerCommand, bool), 64>,
    send_over_connection_receiver: Receiver<'static, ThreadModeRawMutex, u8, 64>,
    main_to_connection_receiver: Receiver<'static, ThreadModeRawMutex, bool, 64>,
    mut blue_led: Output<'static>,
) {
    //This tells the task if it's supposed to try to connect to the network
    let mut active: bool = false;
    let mut connected_to_wifi = false;
    let wifi_connection_timeout = Duration::from_secs(100);
    let mut fan_power: u8 = 0;
    //Buffers for receiving and sending data
    let mut rx_buffer = [0; 4096];
    let mut tx_buffer = [0; 4096];
    let mut buffer: [u8; 1] = [0; 1];
    let mut receive_buffer: [u8; 4096] = [0; 4096];

    //Wait for the connection switch signal to be received
    loop {
        Timer::after_millis(100).await;
        //If active is true, try to connect to the network
        if active {
            //Join Laptop's Hotspot on 2.4Ghz
            if !connected_to_wifi {
                info!("Joining network");

                loop {
                    match with_timeout(
                        wifi_connection_timeout,
                        wifi_control.join_wpa2(WIFI_NETWORK, WIFI_PASSWORD),
                    )
                    .await
                    {
                        Ok(Ok(_)) => {
                            while !stack.is_config_up() {
                                Timer::after_millis(100).await;
                            }
                            info!("DHCP Configured");
                            CONNECTION_READY_CHANNEL.send(true).await;
                            connected_to_wifi = true;
                            break;
                        }
                        Ok(Err(err)) => {
                            info!("Could not join network: {}", err.status);
                            active = false;
                            blue_led.set_low();
                            CONNECTION_READY_CHANNEL.send(false).await;
                            break;
                        }
                        Err(TimeoutError) => {
                            info!("Connection timeout");
                            active = false;
                            blue_led.set_low();
                            CONNECTION_READY_CHANNEL.send(false).await;
                            break;
                        }
                    }
                }
            }

            //Failing to join network will break the process of trying to connect to the network
            if !active {
                continue;
            }
            //Establish TCP Connection
            info!("Creating TCP Socket");
            let mut tcp_socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);

            //Establish TCP connection on port 1234, if it fails,  continue looping until it's successful
            info!("Establishing TCP Connection");
            loop {
                match with_timeout(wifi_connection_timeout, tcp_socket.accept(1234)).await {
                    Ok(Ok(_)) => {
                        info!("TCP connection established");
                        CONNECTION_READY_CHANNEL.send(true).await;
                        break;
                    }
                    Ok(Err(e)) => {
                        warn!("TCP connection couldn't be established:  {:?}", e);
                        active = false;
                        connected_to_wifi = false;
                        blue_led.set_low();
                        wifi_control.leave().await;
                        CONNECTION_READY_CHANNEL.send(false).await;
                        break;
                    }
                    Err(TimeoutError) => {
                        info!("TCP connection timeout");
                        active = false;
                        connected_to_wifi = false;
                        blue_led.set_low();
                        wifi_control.leave().await;
                        CONNECTION_READY_CHANNEL.send(false).await;
                        break;
                    }
                }
            }

            //Listen for signals from the channels
            info!("Listening for signals");

            while active {
                Timer::after_millis(100).await;

                match tcp_socket.may_recv() {
                    true => {}
                    false => {
                        info!("Connection lost");
                        active = false;
                        connected_to_wifi = false;
                        tcp_socket.abort();
                        wifi_control.leave().await;
                        blue_led.set_low();
                        WIFI_CONNECTION_BREAK_CHANNEL.send(true).await;
                    }
                }

                let sig = select3(
                    main_to_connection_receiver.receive(),
                    send_over_connection_receiver.receive(),
                    tcp_socket.read(&mut receive_buffer),
                )
                .await;
                info!("Received signal");
                //Match the signal received from the 3 channels
                match sig {
                    //If the connection switch signal is false, set active to false and turn off the blue led
                    First_3(mode) => match mode {
                        false => {
                            info!(
                                "Switching off connection, we're sending the laptop the 111 code"
                            );
                            buffer[0] = 111;

                            match tcp_socket.write_all(&buffer).await {
                                Ok(_) => {
                                    info!("Sent the 111 code to the laptop");
                                }
                                Err(e) => {
                                    warn!("Couldn't send the 111 code to the laptop: {:?}", e);
                                }
                            };
                            match tcp_socket.flush().await {
                                Ok(_) => {
                                    info!("The laptop probably received the 111 code");
                                    tcp_socket.abort();
                                    match wifi_control.leave().await {
                                        _ => {
                                            info!("Left the network");
                                        }
                                    }
                                }
                                Err(e) => {
                                    warn!("Laptop Couldn't receive the 111 code: {:?}", e);
                                }
                            }
                            connected_to_wifi = false;
                            active = false;
                            blue_led.set_low();
                        }
                        _ => {}
                    },

                    //Receive command to send the power over the connection
                    Second_3(received_power) => {
                        if active {
                            info!("Sending Power");
                            fan_power = received_power;
                            buffer[0] = received_power;
                            info!("Buffer: {:?}", buffer);

                            match tcp_socket.write_all(&buffer).await {
                                Ok(_) => {
                                    info!("Sent new power to desktop app");
                                }
                                //If the write fails, set active to false and turn off the blue led & notify the main task
                                Err(e) => {
                                    warn!("Couldn't send new power to desktop app:  {:?}", e);
                                    active = false;
                                    connected_to_wifi = false;
                                    match wifi_control.leave().await {
                                        _ => {
                                            info!("Left the network");
                                        }
                                    }
                                    blue_led.set_low();
                                    WIFI_CONNECTION_BREAK_CHANNEL.send(true).await;
                                    break;
                                }
                            };
                        }
                    }

                    Third_3(msg_length) => match msg_length {
                        //Received power from the laptop
                        Ok(0) => {}
                        Ok(length) => {
                            info!("Received from laptop: {:?}", msg_length);
                            let received_power_str = from_utf8(&receive_buffer[..length]).unwrap();
                            info!("Received from laptop: {}", received_power_str);
                            let mut received_power = received_power_str.parse::<u8>().unwrap();

                            if received_power == 111 {
                                info!("Connection broken from laptop");
                                active = false;
                                connected_to_wifi = false;
                                tcp_socket.abort();
                                match wifi_control.leave().await {
                                    _ => {
                                        info!("Left the network");
                                    }
                                }
                                blue_led.set_low();
                                WIFI_CONNECTION_BREAK_CHANNEL.send(true).await;
                                break;
                            }

                            info!("Received power: {}", received_power);
                            if received_power > fan_power {
                                power_control_sender
                                    .send((PowerCommand::Increase, false))
                                    .await;
                            } else {
                                power_control_sender
                                    .send((PowerCommand::Decrease, false))
                                    .await;
                            }
                            fan_power = received_power;
                        }
                        Err(e) => {
                            warn!("Couldn't read from TCP socket: {:?}", e);
                            active = false;
                            connected_to_wifi = false;
                            wifi_control.leave().await;
                            blue_led.set_low();
                            WIFI_CONNECTION_BREAK_CHANNEL.send(true).await;
                            break;
                        }
                    },
                }
            }
        } else {
            //active is false, we wait for signal to switch the wifi & blue led on
            match main_to_connection_receiver.receive().await {
                true => {
                    active = true;
                    blue_led.set_high();
                }
                _ => {}
            }
        }
    }
}

//BUTTONS TASKS

#[embassy_executor::task]
async fn button_power_switch(mut power_switch: Debouncer<'static>) {
    loop {
        power_switch.debounce().await;
        SETUP_SWITCH_CHANNEL.send(false).await;
        Timer::after_millis(BUTTONS_TASK_DELAY).await;
    }
}

#[embassy_executor::task]
async fn button_increase_power_pressed(
    mut button_increase: Debouncer<'static>,
    power_control_sender: Sender<'static, ThreadModeRawMutex, (PowerCommand, bool), 64>,
) {
    loop {
        button_increase.debounce().await;
        power_control_sender
            .send((PowerCommand::Increase, true))
            .await;
        Timer::after_millis(BUTTONS_TASK_DELAY).await;
    }
}

#[embassy_executor::task]
async fn button_decrease_power_pressed(
    mut button_decrease: Debouncer<'static>,
    power_control_sender: Sender<'static, ThreadModeRawMutex, (PowerCommand, bool), 64>,
) {
    loop {
        button_decrease.debounce().await;
        power_control_sender
            .send((PowerCommand::Decrease, true))
            .await;
        Timer::after_millis(BUTTONS_TASK_DELAY).await;
    }
}

#[embassy_executor::task]
async fn button_wifi_connection(
    mut button_connect: Debouncer<'static>,
    connection_switch_sender: Sender<'static, ThreadModeRawMutex, bool, 64>,
) {
    loop {
        button_connect.debounce().await;
        info!("Wifi button pressed");
        connection_switch_sender.send(false).await;
        Timer::after_millis(BUTTONS_TASK_DELAY).await;
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    //INITIALIZING VARIABLES
    let mut on: bool = false; //This is the state of the circuit, if it's on or off
    let mut wifi_on: bool = false; //This is the state of the wifi, if it's on or off
    let mut power: u8 = 0; //This is the power of the fans

    // Init peripherals
    let peripherals = embassy_rp::init(Default::default());

    // Start USB logger driver
    let usb_driver = Driver::new(peripherals.USB, Irqs);
    spawner.spawn(logger_task(usb_driver)).unwrap();

    // Link CYW43 firmware
    let fw = include_bytes!("../cyw43-firmware/43439A0.bin");
    let clm = include_bytes!("../cyw43-firmware/43439A0_clm.bin");

    // Init SPI for communication with CYW43
    let pwr = Output::new(peripherals.PIN_23, Level::Low);
    let cs = Output::new(peripherals.PIN_25, Level::High);
    let mut pio = Pio::new(peripherals.PIO0, Irqs);
    let spi = PioSpi::new(
        &mut pio.common,
        pio.sm0,
        pio.irq0,
        cs,
        peripherals.PIN_24,
        peripherals.PIN_29,
        peripherals.DMA_CH0,
    );

    // Start Wi-Fi task
    static STATE: StaticCell<cyw43::State> = StaticCell::new();
    let state = STATE.init(cyw43::State::new());
    let (net_device, mut control, runner) = cyw43::new(state, pwr, spi, fw).await;
    spawner.spawn(wifi_task(runner)).unwrap();

    // Init the device
    control.init(clm).await;
    control
        .set_power_management(cyw43::PowerManagementMode::PowerSave)
        .await;

    let cfg = Config::ipv4_static(embassy_net::StaticConfigV4 {
        address: embassy_net::Ipv4Cidr::new(embassy_net::Ipv4Address::new(192, 168, 137, 160), 24),
        dns_servers: heapless::Vec::new(),
        gateway: None,
    });

    // Generate random seed
    let seed = 0x0123_4567_89ab_cdef;

    // Init network stack
    static STACK: StaticCell<Stack<cyw43::NetDriver<'static>>> = StaticCell::new();
    static RESOURCES: StaticCell<StackResources<2>> = StaticCell::new();
    let stack = &*STACK.init(Stack::new(
        net_device,
        cfg,
        RESOURCES.init(StackResources::<2>::new()),
        seed,
    ));

    // Start network stack task
    spawner.spawn(net_task(stack)).unwrap();

    //Initializing LEDs
    let mut orange_led = Output::new(peripherals.PIN_21, Level::Low);
    let mut green_led = Output::new(peripherals.PIN_17, Level::Low);
    let mut red_led = Output::new(peripherals.PIN_19, Level::Low);
    let mut blue_led = Output::new(peripherals.PIN_26, Level::Low);

    //Start Button tasks with 1 second debouncer

    spawner
        .spawn(button_power_switch(Debouncer::new(
            Input::new(peripherals.PIN_20, Pull::Up),
            DEBOUNCE,
        )))
        .unwrap();

    spawner
        .spawn(button_increase_power_pressed(
            Debouncer::new(Input::new(peripherals.PIN_16, Pull::Up), DEBOUNCE),
            SPEED_CONTROL_CHANNEL.sender(),
        ))
        .unwrap();

    spawner
        .spawn(button_decrease_power_pressed(
            Debouncer::new(Input::new(peripherals.PIN_18, Pull::Up), DEBOUNCE),
            SPEED_CONTROL_CHANNEL.sender(),
        ))
        .unwrap();

    spawner
        .spawn(button_wifi_connection(
            Debouncer::new(Input::new(peripherals.PIN_22, Pull::Up), DEBOUNCE),
            WIFI_BTN_SWITCH_CHANNEL.sender(),
        ))
        .unwrap();

    //Start the exchange over connection task
    spawner
        .spawn(exchange_over_connection(
            control,
            stack,
            SPEED_CONTROL_CHANNEL.sender(),
            SEND_OVER_CONNECTION_CHANNEL.receiver(),
            WIFI_MAIN_SWITCH_CHANNEL.receiver(),
            blue_led,
        ))
        .unwrap();

    // INIT LCD

    let mut displayed_sentence = String::<32>::from_str("State: OFF").unwrap();

    let sda = peripherals.PIN_8;
    let scl = peripherals.PIN_9;

    let mut i2c = I2c::new_async(peripherals.I2C0, scl, sda, Irqs, I2cConfig::default());
    let mut sender = sender::I2cSender::new(&mut i2c, LCD_ADDR);
    let lcd_config = lcd::Config::default();
    let mut delayer = Delay;
    let mut lcd = lcd::Lcd::new(&mut sender, &mut delayer, lcd_config, DISPLAY_FREQUENCY);
    lcd.set_cursor_blink_state(State::Off);

    lcd.set_cursor_pos((0, 0));
    lcd.write_str_to_cur(&displayed_sentence);

    //INIT PWM

    let _direction_motors = Output::new(peripherals.PIN_1, Level::High);

    let mut config_pwm_motors: PwmConfig = Default::default();
    config_pwm_motors.top = TOP;
    config_pwm_motors.compare_a = 0x0000;

    let mut pwm_motors = Pwm::new_output_a(
        peripherals.PWM_SLICE0,
        peripherals.PIN_0,
        config_pwm_motors.clone(),
    );

    //Start main loop and listen for signals from channels & handle them
    loop {
        lcd.set_cursor_blink_state(State::Off);

        Timer::after_millis(100).await;

        let sig = select4(
            WIFI_CONNECTION_BREAK_CHANNEL.receive(),
            WIFI_BTN_SWITCH_CHANNEL.receive(),
            SETUP_SWITCH_CHANNEL.receive(),
            SPEED_CONTROL_CHANNEL.receive(),
        )
        .await;

        match sig {
            First_4(_) => {
                //If the connection is broken, turn off the blue led & set wifi to off
                wifi_on = false;

                displayed_sentence = String::<32>::try_from("WIFI: Off").unwrap();
                lcd.set_cursor_pos((0, 1));
                lcd.write_str_to_cur(&displayed_sentence);
            }

            Second_4(_) => {
                if on == true {
                    lcd.clean_display();
                    displayed_sentence = String::<32>::try_from("Connecting...").unwrap();
                    lcd.set_cursor_pos((0, 0));
                    lcd.write_str_to_cur(&displayed_sentence);

                    //If the wifi button is pressed, switch wifi on or off
                    wifi_on = !wifi_on;
                    WIFI_MAIN_SWITCH_CHANNEL.send(wifi_on).await;

                    if wifi_on {
                        //First for connecting to wifi network
                        match CONNECTION_READY_CHANNEL.receive().await {
                            true => {
                                lcd.clean_display();
                                displayed_sentence = String::<32>::try_from("WIFI: Ready").unwrap();
                                lcd.set_cursor_pos((0, 0));
                                lcd.write_str_to_cur(&displayed_sentence);
                            }
                            false => {
                                wifi_on = false;
                                WIFI_MAIN_SWITCH_CHANNEL.send(false).await;

                                lcd.clean_display();

                                displayed_sentence = match_power(power.clone());

                                lcd.set_cursor_pos((0, 0));
                                lcd.write_str_to_cur(&displayed_sentence);

                                displayed_sentence = String::<32>::try_from("WIFI: Off").unwrap();
                                lcd.set_cursor_pos((0, 1));
                                lcd.write_str_to_cur(&displayed_sentence);
                                continue;
                            }
                        }

                        //Second for TCP connection
                        match CONNECTION_READY_CHANNEL.receive().await {
                            true => {
                                lcd.clean_display();

                                displayed_sentence = match_power(power.clone());

                                lcd.set_cursor_pos((0, 0));
                                lcd.write_str_to_cur(&displayed_sentence);

                                displayed_sentence = String::<32>::try_from("WIFI: On").unwrap();
                                lcd.set_cursor_pos((0, 1));
                                lcd.write_str_to_cur(&displayed_sentence);

                                info!("Sending power to wifi task");
                                Timer::after_millis(400).await;
                                SEND_OVER_CONNECTION_CHANNEL.send(power).await;
                            }
                            false => {
                                wifi_on = false;
                                WIFI_MAIN_SWITCH_CHANNEL.send(false).await;
                                displayed_sentence = String::<32>::try_from("WIFI: Off").unwrap();
                                lcd.set_cursor_pos((0, 1));
                                lcd.write_str_to_cur(&displayed_sentence);
                            }
                        }
                    } else {
                        displayed_sentence = String::<32>::try_from("WIFI: Off").unwrap();
                        lcd.set_cursor_pos((0, 1));
                        lcd.write_str_to_cur(&displayed_sentence);
                    }
                }
            }

            Third_4(_) => {
                on = !on;

                config_pwm_motors.compare_a = 0x0000;

                pwm_motors.set_config(&config_pwm_motors);

                lcd.clean_display();

                if on {
                    orange_led.set_high();

                    displayed_sentence = String::<32>::try_from("State: On").unwrap();

                    lcd.set_cursor_pos((0, 0));
                    lcd.write_str_to_cur(&displayed_sentence);

                    Timer::after(Duration::from_secs(2)).await;

                    displayed_sentence = String::<32>::try_from("Power: Low").unwrap();
                    lcd.clean_display();

                    lcd.set_cursor_pos((0, 0));
                    lcd.write_str_to_cur(&displayed_sentence);

                    lcd.set_cursor_pos((0, 1));
                    displayed_sentence = String::<32>::try_from("WIFI: Off").unwrap();
                    lcd.write_str_to_cur(&displayed_sentence);
                } else {
                    orange_led.set_low();

                    wifi_on = false;
                    WIFI_MAIN_SWITCH_CHANNEL.send(false).await;
                    power = 0;

                    displayed_sentence = String::<32>::try_from("State: Off").unwrap();

                    lcd.set_cursor_pos((0, 0));
                    lcd.write_str_to_cur(&displayed_sentence);
                    Timer::after_secs(2).await;
                }
            }

            Fourth((power_command, send_wifi)) => {
                //If the power command is received, increase or decrease the power of the fans, update the display & send the power over the connection if it's connected
                if on == true {
                    match power_command {
                        PowerCommand::Increase => {
                            if power < 100 {
                                match power {
                                    0 => {
                                        power = 80;
                                        config_pwm_motors.compare_a = 0x6000;
                                    }
                                    80 => {
                                        power = 100;
                                        config_pwm_motors.compare_a = 0x8000;
                                    }
                                    _ => {}
                                }

                                green_led.set_high();
                                Timer::after(SPEED_CHANGE_DELAY).await;
                                green_led.set_low();
                            }
                        }
                        PowerCommand::Decrease => {
                            if power > 0 {
                                match power {
                                    80 => {
                                        power = 0;
                                        config_pwm_motors.compare_a = 0x0000;
                                    }
                                    100 => {
                                        power = 80;
                                        config_pwm_motors.compare_a = 0x6000;
                                    }
                                    _ => {}
                                }
                                red_led.set_high();
                                Timer::after(SPEED_CHANGE_DELAY).await;
                                red_led.set_low();
                            }
                        }
                    }

                    pwm_motors.set_config(&config_pwm_motors);

                    lcd.clean_display();

                    displayed_sentence = match_power(power.clone());

                    lcd.set_cursor_pos((0, 0));
                    lcd.write_str_to_cur(&displayed_sentence);

                    if wifi_on & send_wifi {
                        displayed_sentence = String::<32>::try_from("WIFI: On").unwrap();

                        SEND_OVER_CONNECTION_CHANNEL.send(power).await;
                    } else if !wifi_on {
                        displayed_sentence = String::<32>::try_from("WIFI: Off").unwrap();
                    } else {
                        displayed_sentence = String::<32>::try_from("WIFI: On").unwrap();
                    }
                    lcd.set_cursor_pos((0, 1));
                    lcd.write_str_to_cur(&displayed_sentence);
                }
            }
        }
    }
}
