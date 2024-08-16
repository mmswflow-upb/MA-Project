# Microprocessor Architecture Project-README
For more details check the [presentation](PRESENTATION.md) and how to [set it up](PROJECT-SETUP.md).

## What is this project about?
We had to create a device using electronic components such as sensors, motors, etc and control them using an MCU of 
our choice. The firmware had to be written in Rust using any framework we wanted, I've decided to use the 
[RP Pico W](https://www.raspberrypi.com/documentation/microcontrollers/pico-series.html#raspberry-pi-pico-w) with 
the [embassy](https://embassy.dev/) framework because it was the same one we had been using during the labs. We also had 
to fork the repository of the [course's website](https://embedded-rust-101.wyliodrin.com/) and add our own project 
presentations, then we had to create pull requests that were tested before merging.

## How did it go?
At first I was unsure of what I wanted to create, but then I tried to go with one idea and develop it incrementally, it was 
tough to work with embassy due to my unfamiliarity with the Rust language, it's a pretty tough language to learn and we had
little time to get used to it.

One of the obstacles was trying to work with modules or libraries, which would help me with 
operating the peripherals connected to my MCU such as the LCD screen or the wifi chip, 
this is because the community is pretty small and I had to try a couple different modules 
until I found an actual working one.

The toughest part was creating the logic for my project, since I had to expect input both 
from a tactile physical remote and an application on the PC simultaneously. This part took
the longest time to finish, because I had to connect my MCU to my laptop each time I tested
the feature.

## What did I learn?
I learned how to work with electronic components, MCUs, I studied communication 
protocols between integrated circuits, I learned some rust along with the embasssy framework, 
I also learned more about app development (with Python), and I learned how to use KiCad and GitHub.

Overall, it was a wonderful learning experience 💪💪
