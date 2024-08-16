from ttkbootstrap import *
from ttkbootstrap.constants import *
from ttkbootstrap.widgets import *
from ttkbootstrap.style import *
from PIL import Image
from tkinter import PhotoImage

import keyboard

import time
import socket
import threading

#CONSTANTS:
THEME = "DARK"
ICON = "assets\\icon.png"
LABEL_FONT = ("Arial", 15)
LIGHT_THEME_ICON = "assets\\dark_theme.png"
DARK_THEME_ICON = "assets\\light_theme.png"
INCREASE_COMBO = {'ctrl', 'i'}
DECREASE_COMBO = {'ctrl', 'd'}
#NETWORKING CLASS

class CoolingPadClient:
    def __init__(self,ipaddress,port ):
        self.pico_port = port
        self.pico_ip_address = ipaddress
        self.socket = None
        self.connected = False
        self.check_button_var = None
        self.listening_thread = None
        self.key_listening_thread = None
        self.current_key_set = set()
        self.power = 0
        self.closing = False
        self.debounce = False
        self.listening_key = False


    def run_app(self):

        #UI METHODS
        def close_app():
            if self.debounce == True:
                return   

            #If we're connected, disconnect
            if (self.connected == True):

                print("Closing app-Sending power off signal")

                send_power(111, True)
                self.closing = True

                print("Closing app- Unhooking all key events")
                if self.key_listening_event != None:
                    print("Closing app- Triggered key listening event")
                    self.key_listening_event.set()
                    keyboard.unhook_all()

                
            self.debounce = False
            exit()


        def connect_button():
            
            if self.debounce == True:
                return    
            
            self.debounce = True
            #If we're connected, disconnect
            if (self.connected == True):

                
                send_power(111)
                self.connected = False
                connect_mcu_button.config(text="Not Connected")
                self.check_button_var.set(False)
                control_menu_frame.pack_forget()
                not_connected_label.pack()
                self.socket.close()
                if self.key_listening_event != None:
                    self.key_listening_event.set()
                self.debounce = False
                keyboard.unhook_all()

                return


            #If we're not connected, try to connect

            self.socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            connect_mcu_button.config(text="Trying to Connect")

            
            threading.Thread(target=connect_thread).start()


        def increase_power():

            if self.power == 0:
                send_power(80)
            elif self.power == 80:
                send_power(100)


        def decrease_power():

            if self.power == 100:
                send_power(80)
            elif self.power == 80:
                send_power(0)


        def change_theme():
            if root.style.theme.__getattribute__("name") == "vapor":
                root.style.theme_use("pulse")
                theme_button.config(image=light_theme_photo_image)

            else:
                root.style.theme_use("vapor")
                theme_button.config(image=dark_theme_photo_image)     

        def update_power_label():
            if self.power == 0:
                power_label.config(text="Power: Low")
            elif self.power == 80:
                power_label.config(text="Power: Medium")
            elif self.power == 100:
                power_label.config(text="Power: High")

        #Keyboard input (shortcuts)
                         
        def detect_key_combination(event):
           
            
            name = event.name
            if event.event_type == keyboard.KEY_DOWN:
                
                self.current_key_set.add(name)
                if self.current_key_set == INCREASE_COMBO and self.connected:

                    increase_power()
                    
                elif self.current_key_set == DECREASE_COMBO and self.connected:
                    
                    decrease_power()


            elif event.event_type == keyboard.KEY_UP:
                self.current_key_set.discard(name)


        def listen_key_combo_shortcut_thread():
            
            # Hook to all key events
            print("Hooking to all key events")
            keyboard.hook(detect_key_combination)
            self.key_listening_event = threading.Event()
            self.key_listening_event.wait()
            print("unhooking all key events")
            keyboard.unhook_all()
            self.key_listening_event = None
                
                    
        def listen_key_combo_shortcut():
            self.key_listening_thread = threading.Thread(target=listen_key_combo_shortcut_thread)
            self.key_listening_thread.start()

       

        #NETWORKING METHODS
        def connect_thread():

            try:
                
                #self.socket.connect((self.socket.gethostbyname("CoolingPadPico"),self.pico_port))
                self.socket.connect((self.pico_ip_address,self.pico_port))
                
                #self.socket.connect(("192.168.137.248",self.pico_port))
                self.connected = True

            except socket.error as e:
                print(f"Couldn't connect to {self.pico_ip_address} on port {self.pico_port}")
                self.connected = False
                self.debounce = False
                

            if self.connected == True:
                connect_mcu_button.config(text="Connected")
                
                self.check_button_var.set(True)   
                not_connected_label.pack_forget()
                control_menu_frame.pack()
                not_connected_label.pack_forget()
                control_menu_frame.pack()
                receive_data()
                listen_key_combo_shortcut()

            else:    
                connect_mcu_button.config(text="Not Connected")
                self.check_button_var.set(False)
                
                control_menu_frame.pack_forget()
                not_connected_label.pack()
            
            self.debounce = False  

        def send_power_thread(power, closing):
            
            if (self.connected & self.debounce == False):
                self.debounce = True
                try:
                    self.socket.sendall(str(power).encode())
                    time.sleep(1)
                    if power != 111:
                        self.power = power
                    if closing == False:
                        update_power_label()
                        self.debounce = False
                    if power == 111:

                        self.socket.close()
                    return True

                except socket.error as e:
                    self.debounce = False
                    return False
                
            return False

        def send_power(power, closing=False):
            
            threading.Thread(target=send_power_thread, args=(power,closing)).start()

        def receive_data_thread():
            while self.connected:

                time.sleep(1)
                
                try: 
                    if self.closing == True:
                        return
                
                    self.power = self.socket.recv(1)[0]
                    
                    if self.power == 111:
                        
                        self.connected = False
                        self.check_button_var.set(False)
                        connect_mcu_button.config(text="Not Connected")
                        control_menu_frame.pack_forget()
                        not_connected_label.pack()
                        self.socket.close()
                        if self.key_listening_event != None:
                            self.key_listening_event.set()
                        self.power = 0
                        return
                    
                    threading.Thread(target=update_power_label, ).start()                   

                    
                except socket.error as e:
                    if self.closing == True:
                        return
                    
                    self.check_button_var.set(False)
                    connect_mcu_button.config(text="Not Connected")
                    control_menu_frame.pack_forget()
                    not_connected_label.pack()
                    if self.key_listening_event != None:
                        self.key_listening_event.set()

                    self.connected = False
                    continue
                    
        
        def receive_data():
            
            self.listening_thread = threading.Thread(target=receive_data_thread)
            self.listening_thread.start()

        #WIDGETS:

        # Create the main window
        root = Window(themename="vapor")
        root.title("Cooling Station Client")
        root.geometry("500x500")
        root.iconphoto(False, PhotoImage(file=ICON))
        root.resizable(False, False)
        root.protocol("WM_DELETE_WINDOW", close_app)
        #Creating variables for widgets:
        self.check_button_var = BooleanVar(value=False)
        #Creating Images:

        dark_theme_img = Image.open(DARK_THEME_ICON).resize((30,30),Image.Resampling.LANCZOS)
        dark_theme_photo_image = ImageTk.PhotoImage(dark_theme_img)


        light_theme_img = Image.open(LIGHT_THEME_ICON).resize((30,30),Image.Resampling.LANCZOS)
        light_theme_photo_image = ImageTk.PhotoImage(light_theme_img)

        #Creating the frames
        row_1_frame = Frame(root)
        control_menu_frame = Frame(row_1_frame)
        util_menu_frame = Frame(root)

        #Creating the labels
        title = Label(root, text="🌀Cooling Station Driver", font=LABEL_FONT, bootstyle="info")
        not_connected_label = Label(row_1_frame, text="Awaiting Connection", font=LABEL_FONT, bootstyle="warning")
    
        power_label = Label(control_menu_frame, bootstyle="info", text="Power: Low", font=LABEL_FONT)
        
        #Creating the buttons
        connect_mcu_button =  Checkbutton(util_menu_frame, text="Not Connected",bootstyle="round-toggle-info", command=connect_button, variable=self.check_button_var)
        theme_button = Button(util_menu_frame, style="primary outline",image=dark_theme_photo_image ,command=change_theme)

        increase_power_button = Button(control_menu_frame, text="Increase Power", style="success", command=increase_power)
        decrease_power_button = Button(control_menu_frame, text="Decrease Power", style="danger", command=decrease_power)

       
        #LAYOUTS:

        #Window Layout:
        root.rowconfigure((0,2),weight=1)
        root.rowconfigure(1, weight = 4)
        root.columnconfigure(0, weight = 1)

        #Frames Layouts
        row_1_frame.grid(row=1, column=0, sticky="nsew")
        control_menu_frame.columnconfigure((0,1), weight=1)
        control_menu_frame.rowconfigure((0,1), weight=1)

        util_menu_frame.grid(row=2, column=0, sticky="sew")
        util_menu_frame.columnconfigure((0,1,2,3), weight=1)

        #Labels Layouts
        title.grid(row=0, column=0, sticky="ns")
        not_connected_label.pack()
        
        power_label.grid(row=0, column=0, sticky="ns",columnspan=2)

        #Buttons Layouts
        connect_mcu_button.grid(row=0, column=3, sticky="se")
        connect_mcu_button.config(padding=20)
        theme_button.grid(row=0, column=0, sticky="w")


        increase_power_button.grid(row=1,column=0, sticky="nswe", pady=5)
        decrease_power_button.grid(row=1, column=1, sticky="nswe", pady=5)

        

        # Run the Tkinter event loop
        root.mainloop()

        

if (__name__ == "__main__"):
    client = CoolingPadClient("192.168.137.160",1234)
    client.run_app()