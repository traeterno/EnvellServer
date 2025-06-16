#![allow(static_mut_refs, non_upper_case_globals, non_snake_case)]

use std::time::{Duration, Instant};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::io::{ErrorKind, Read, Write};
use std::fs::File;
use std::collections::HashMap;

use ini::Ini;

#[repr(u8)]
#[derive(Clone, Copy)]
enum Request
{
	NewPlayer = 0,
	Disconnected = 1,
	CharSelected = 2,
	PlayersList = 3,
	Chat = 4,
	Ready = 5,
	Update = 6,
	Invalid = 255
}

impl Request
{
	pub fn isValid(code: u8) -> bool
	{
		return code == 255u8 || code == code.clamp(0, 4);
	}
}

struct PlayerEntry { pub name: String, pub class: String}

struct Client
{
	id: u8,
	name: String,
	class: String,
	ip: SocketAddr,
	tcp: Option<TcpStream>
}

impl Client
{
	pub fn serialized(&self) -> String
	{
		self.id.to_string() + ":" + &self.name + "|" + &self.class
	}
	pub fn toString(&self) -> String
	{
		self.id.to_string() + ":" + &self.name + "|" + &Server::getClass(&self.class)
	}
}

struct Server
{
	listener: TcpListener,
	shouldDebug: bool,
	saveFile: String,
	clients: Vec<Client>,
	classes: HashMap<String, String>,
	players: HashMap<IpAddr, PlayerEntry>,
	tokens: Vec<bool>,
	maxPlayersCount: u8,
	deletePlayer: usize,
	ready: bool,
	udp: Option<UdpSocket>,
	state: [Vec<u8>; 5],
	stateUpdateRate: Duration
}

impl Server
{
	fn default() -> Server
	{
		Server
		{
			listener: TcpListener::bind("0.0.0.0:2018").unwrap(),
			shouldDebug: false,
			saveFile: String::from("config.ini"),
			clients: vec![],
			classes: HashMap::new(),
			players: HashMap::new(),
			tokens: vec![],
			maxPlayersCount: 1,
			deletePlayer: usize::MAX,
			ready: false,
			udp: None,
			state: [const { vec![] }; 5],
			stateUpdateRate: Duration::new(0, 0)
		}
	}

	fn getInstance() -> &'static mut Server
	{
		static mut i: Option<Server> = None;
		unsafe { if i.is_none() { i = Some(Server::default()); } i.as_mut().expect("Server singleton is not initialized") }
	}

	fn loadConfig()
	{
		let i = Server::getInstance();

		let mut res = File::open(&i.saveFile);
		if res.is_err() { panic!("Failed to open save file from '{}': {:?}", i.saveFile, res.unwrap_err()); }

		let res = Ini::read_from(res.as_mut().unwrap());
		if res.is_err() { panic!("Failed to load save file from '{}': {:?}", i.saveFile, res.unwrap_err()); }

		let doc = res.unwrap();

		if let Some(settings) = doc.section(Some("settings"))
		{
			i.shouldDebug = settings.get("debug").unwrap_or("false").parse().unwrap();
			i.maxPlayersCount = settings.get("maxPlayersCount").unwrap_or("1").parse().unwrap();
			i.stateUpdateRate = Duration::from_secs_f32(
				1.0 / settings.get("stateUpdateRate").unwrap_or("50").parse::<f32>().unwrap()
			);
		}

		Server::debug(format!("Debug enabled"));
		Server::debug(format!("Max players count: {}", i.maxPlayersCount));

		if let Some(classes) = doc.section(Some("classes"))
		{
			for (id, name) in classes
			{
				i.classes.insert(id.to_string(), name.to_string());
				Server::debug(format!("Class: id '{}', name '{}'", id, name));
			}
		}

		if let Some(players) = doc.section(Some("players"))
		{
			for (ip, entry) in players
			{
				let data: Vec<&str> = entry.split(" ").collect();
				i.players.insert(
					ip.parse().unwrap_or(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0))),
					PlayerEntry
					{
						name: String::from(data[0]),
						class: String::from(data[1])
					}
				);
				Server::debug(format!("Player: ip '{}', name '{}', class '{}'", ip, data[0], data[1]));
			}
		}
	}

	fn saveConfig()
	{
		let i = Server::getInstance();

		let mut ini = Ini::new();

		ini.with_section(Some("settings"))
			.add("debug", i.shouldDebug.to_string())
			.add("maxPlayersCount", i.maxPlayersCount.to_string());

		let mut classes = ini.with_section(Some("classes"));
		for (id, name) in &i.classes
		{
			classes.add(id, name);
		}

		let mut players = ini.with_section(Some("players"));
		for c in &i.clients
		{
			players.add(
				c.ip.ip().to_string(),
				c.name.clone() + " " + if c.class.is_empty() { "noname" } else { &c.class }
			);
		}
		
		Server::debug(format!("Saved: {:?}", ini.write_to_file(&i.saveFile)));
	}

	fn getPlayer(ip: IpAddr) -> PlayerEntry
	{
		let mut entry = PlayerEntry { name: String::new(), class: String::new() };
		if let Some(x) = Server::getInstance().players.get(&ip)
		{
			entry.name = x.name.clone();
			entry.class = x.class.clone();
			return entry;
		}
		else { return entry; }
	}

	fn getClass(id: &String) -> String
	{
		Server::getInstance().classes.get(id).unwrap_or(&"NoName".to_string()).to_string()
	}

	fn getEmptyID() -> u8
	{
		let i = Server::getInstance();
		for index in 0..i.tokens.len()
		{
			if !i.tokens[index]
			{
				i.tokens[index] = true;
				return (index + 1) as u8;
			}
		}
		return 0u8;
	}

	fn newPlayer(tcp: TcpStream)
	{
		let i = Server::getInstance();
		let _ = tcp.set_nonblocking(true);
		let ip = tcp.local_addr().unwrap();
		let p = Server::getPlayer(ip.ip());
		let id = Server::getEmptyID();
		let mut client = Client { id, name: p.name.clone(), class: p.class.clone(), ip, tcp: Some(tcp) };
		let out = client.serialized();
		Server::sendTCP(&mut client, Request::NewPlayer, &out);
		i.clients.push(client);
	}

	fn debug(out: String)
	{
		if !Server::getInstance().shouldDebug { return; }
		println!("{}", out);
	}

	fn startListener()
	{
		Server::loadConfig();
		let i = Server::getInstance();
		let _ = i.listener.set_nonblocking(true);
		i.tokens.resize(i.maxPlayersCount as usize, false);
		println!("Started server on port {}", i.listener.local_addr().unwrap().port());
		for client in i.listener.incoming()
		{
			if i.ready { break; }
			match client
			{
				Ok(x) => { Server::newPlayer(x); }
				Err(_) => {}
			}
		}
	}

	fn sendTCP(c: &mut Client, req: Request, data: &String)
	{
		let res = c.tcp.as_mut().unwrap().write(&[
			&[req as u8],
			data.as_bytes()
		].concat());
		if res.is_err()
		{
			let err = res.unwrap_err();
			if err.kind() != ErrorKind::WouldBlock
			{
				println!("Failed to send data to {}: {err:?}", c.ip);
			}
			return;
		}
		Server::debug(format!("Sent to {}: {}, {data}", c.ip, req as u8));
	}

	fn broadcastTCP(req: Request, data: String)
	{
		for c in &mut Server::getInstance().clients
		{
			Server::sendTCP(c, req, &data);
		}
	}

	fn receiveTCP(c: &mut Client) -> (u8, String)
	{
		let buffer = &mut [0u8; 1024];
		let res = c.tcp.as_mut().unwrap().read(buffer);
		let i = Server::getInstance();
		if res.is_err()
		{
			let err = res.unwrap_err();
			if err.kind() != ErrorKind::WouldBlock
			{
				println!("Failed to receive data from {}: {err:?}", c.ip);
				for index in 0..i.clients.len()
				{
					if c.id == i.clients[index].id { i.deletePlayer = index; break; }
				}
			}
			return (255u8, String::new());
		}
		let len = res.unwrap();
		if len == 0 { return (255u8, String::new()); }
		let req = buffer[0];
		let data = String::from_utf8_lossy(&buffer[1..len]).to_string();
		if !Request::isValid(req)
		{
			Server::debug(format!("Client {} sent invalid request", c.ip));
			for index in 0..i.clients.len()
			{
				if c.id == i.clients[index].id { i.deletePlayer = index; break; }
			}
			return (255u8, String::new());
		}
		Server::debug(format!(
			"Received from {}: {req}/{data}",
			if c.name.is_empty() { c.ip.to_string() } else { c.name.clone() }
		));
		return (req, data);
	}
	

	fn getPlayersList() -> String
	{
		let i = Server::getInstance();
		let mut out = String::new();
		for i1 in 0..i.clients.len()
		{
			out += &(i.clients[i1].toString() + ";");
		}
		return out;
	}

	fn execute(c: &mut Client, cmd: String)
	{
		if cmd.is_empty() { return; }
		Server::debug(format!("Player {} executed '{cmd}'", c.name));
		let args: Vec<&str> = cmd.split(" ").collect();
		let i = Server::getInstance();
		Server::debug(format!("Command arguments: {args:?}"));
		if args[0] == "start"
		{
			i.ready = true;
			Server::debug(format!("Player {} started the game.", c.name));
		}
		if args[0] == "kick"
		{
			for index in 0..i.clients.len()
			{
				if i.clients[index].name == args[1] { i.deletePlayer = index; break; }
			}
			if i.deletePlayer != usize::MAX
			{
				let n = i.clients[i.deletePlayer].name.clone();
				Server::debug(format!("Player {} kicked out player {}", c.name, n));
			}
		}
		if args[0] == "kickID"
		{
			let res = args[1].parse::<u8>();
			if res.is_err()
			{
				Server::debug(format!("Player {} entered wrong player ID", c.name));
				return;
			}
			for index in 0..i.clients.len()
			{
				if i.clients[index].id == *res.as_ref().unwrap() { i.deletePlayer = index; break; }
			}
			if i.deletePlayer != usize::MAX
			{
				let n = res.unwrap();
				Server::debug(format!("Player {} kicked out player {}", c.name, n));
			}
		}
		if args[0] == "update"
		{
			if args[1] == "playersList"
			{
				Server::broadcastTCP(Request::PlayersList, Server::getPlayersList());
				Server::debug(format!("Player {} requested to update players list", c.name));
			}
		}
		if args[0] == "stop" || args[0] == "close"
		{
			Server::broadcastTCP(Request::Chat, format!("[Игрок {} не имеет прав.]", c.name));
			Server::debug(format!("Player {} tried to stop the server.", c.name));
		}
	}

	pub fn start()
	{
		let i = Server::getInstance();
		std::thread::spawn(Server::startListener);

		while !i.ready
		{
			for index in 0..i.clients.len()
			{
				let c = i.clients.get_mut(index).unwrap();
				let (req, data) = Server::receiveTCP(c);

				if req == Request::NewPlayer as u8
				{
					c.name = data;
					Server::broadcastTCP(Request::NewPlayer, c.toString());
				}
				else if req == Request::Disconnected as u8 { i.deletePlayer = index; }
				else if req == Request::CharSelected as u8
				{
					c.class = data;
					let name = Server::getClass(&c.class);
					Server::broadcastTCP(Request::CharSelected,
						c.id.to_string() + &name
					);
					Server::debug(format!("Player {} selected char '{}'", c.name, name));
				}
				else if req == Request::PlayersList as u8
				{
					let out = Server::getPlayersList();
					Server::debug(format!("Player {} requested players list", c.name));
					Server::broadcastTCP(Request::PlayersList, out);
				}
				else if req == Request::Chat as u8
				{
					let text = &data[1..data.len()];
					if text.chars().nth(0).unwrap() == '/' { Server::execute(c, text[1..text.len()].to_string()); }
					else
					{
						let msg = c.name.clone() + ": " + text;
						Server::broadcastTCP(Request::Chat, msg.clone()); 
						Server::debug(format!("Message - {}", msg));
					}
				}
				else if req == Request::Invalid as u8 {}
			}

			if i.deletePlayer != usize::MAX
			{
				let c = &i.clients[i.deletePlayer];
				if !c.name.is_empty()
				{
					Server::debug(format!("Player {} has disconnected", c.name));
				}
				else
				{
					Server::debug(format!("Client {} has disconnected", c.ip));
				}
				i.tokens[(c.id - 1) as usize] = false;
				i.clients.remove(i.deletePlayer);
				i.deletePlayer = usize::MAX;
				Server::broadcastTCP(Request::PlayersList, Server::getPlayersList());
			}
		}

		Server::saveConfig();

		let mut ready = String::new();
		for c in &mut i.clients
		{
			ready.push_str(&c.id.to_string());
			ready.push_str(&c.class);
			ready.push('\n');
		}

		Server::broadcastTCP(Request::Ready, ready);

		for c in &mut i.clients
		{
			c.tcp = None;
		}
		
		Server::gameLoop();
	}

	fn sendUDP(c: &mut Client, req: Request, data: &String)
	{
		let i = Server::getInstance();
		let _ = i.udp.as_mut().unwrap().send_to(
			&[
				&[req as u8],
				data.as_bytes()
			].concat(),
			c.ip
		);
	}

	fn sendRaw(c: &mut Client, req: Request, data: &[u8])
	{
		let i = Server::getInstance();
		let _ = i.udp.as_mut().unwrap().send_to(
			&[
				&[req as u8],
				data
			].concat(),
			c.ip
		);
	}

	// fn broadcastUDP(req: Request, data: &String)
	// {
	// 	let i = Server::getInstance();
	// 	for c in &mut i.clients
	// 	{
	// 		Server::sendUDP(c, req, data);
	// 	}
	// }

	fn broadcastRaw(req: Request, data: &[u8])
	{
		let i = Server::getInstance();
		Server::debug(format!("Broadcasting {} bytes to {} clients", data.len(), i.clients.len()));
		for c in &mut i.clients
		{
			Server::sendRaw(c, req, data);
		}
	}

	fn receiveRaw() -> (u8, u8, Vec<u8>)
	{
		let i = Server::getInstance();
		if i.udp.is_none() { return (0u8, 255u8, vec![]); }
		let buffer = &mut [0u8; 1024];
		let res = i.udp.as_mut().unwrap().recv_from(buffer);
		if res.is_err() { return (0u8, 255u8, vec![]); }

		let recv = res.unwrap();
		let req = buffer[0];
		let data = buffer[1..recv.0].to_vec();
		for c in &i.clients
		{
			if c.ip == recv.1
			{
				return (c.id, req, data);
			}
		}
		(0u8, 255u8, vec![])
	}

	fn receiveUDP() -> (u8, u8, String)
	{
		let i = Server::getInstance();
		if i.udp.is_none() { return (0u8, 255u8, String::new()); }
		let buffer = &mut [0u8; 1024];
		let res = i.udp.as_mut().unwrap().recv_from(buffer);
		if res.is_err() { return (0u8, 255u8, String::new()); }

		let recv = res.unwrap();
		let req = buffer[0];
		let data = String::from_utf8_lossy(&buffer[1..recv.0]).to_string();
		if req == Request::Ready as u8
		{
			for c in &mut i.clients
			{
				if c.id == data.parse().unwrap_or(0)
				{
					c.ip = recv.1;
					return (c.id, req, data);
				}
			}
		}
		for c in &i.clients
		{
			if c.ip == recv.1
			{
				return (c.id, req, data);
			}
		}
		(0u8, 255u8, String::new())
	}
	
	fn reload()
	{
		let i = Server::getInstance();
		let playersCount = i.clients.len();
		let mut ready = HashMap::<u8, ()>::new();
		while ready.len() != playersCount
		{
			let (id, _, _) = Server::receiveUDP();
			if id == 0 { Server::debug(format!("Some players have lost connection.")); break; }
			if ready.insert(id, ()).is_some() { continue; }
			for c in &mut i.clients
			{
				if c.id == id
				{
					Server::debug(format!("Player {} reconnected!", c.name));
					Server::sendUDP(c, Request::Ready, &String::new());
				}
			}
		}
	}

	fn receiveLoop()
	{
		let i = Server::getInstance();
		loop
		{
			/* todo:
				server receives data several times from clients before it sends everything back,
				so you have to check if the player has already sent smth and overwrite the packet
				(possibly use hashmap with .insert())
			*/
			let data = Server::receiveRaw();
			// if data.0 == 0
			// {
			// 	i.updateState = false;
			// 	while !i.updateState {}
			// 	i.state.clear();
			// 	i.state.append(&mut order.to_le_bytes().to_vec());
			// 	order += 1;
			// 	continue;
			// }
			// i.state.push(data.0);
			// i.state.append(&mut data.2);
			if data.0 == 0 { continue; }
			i.state[(data.0 - 1) as usize] = data.2;
		}
	}

	fn broadcastGameState(order: u32)
	{
		let mut data = Vec::<u8>::new();
		Server::debug(format!("Packet ID: {order}"));
		data.append(&mut order.to_le_bytes().to_vec());
		let i = Server::getInstance();
		for id in 0..5
		{
			let s = &mut i.state[id];
			Server::debug(format!("ID: {}; data: {}", id + 1, s.len()));
			if s.len() == 0 { continue; }
			data.push((id + 1) as u8);
			data.append(s);
			s.clear();
		}
		Server::broadcastRaw(Request::Update, &data);
	}

	fn gameLoop()
	{
		let i = Server::getInstance();
		i.udp = Some(UdpSocket::bind("0.0.0.0:2018").unwrap());
		let udp = i.udp.as_mut().unwrap();
		let _ = udp.set_write_timeout(Some(Duration::from_secs(10)));
		let _ = udp.set_read_timeout(Some(Duration::from_secs(10)));
		
		Server::debug(format!("Waiting for players..."));
		Server::reload();
		Server::debug(format!("Game started. Good luck!"));

		let mut order = 0u32;
		
		let _ = udp.set_write_timeout(Some(Duration::from_millis(10)));
		let _ = udp.set_read_timeout(Some(Duration::from_millis(10)));
		std::thread::spawn(Server::receiveLoop);
		let mut lastSend = Instant::now();
		loop
		{
			if lastSend.elapsed() > i.stateUpdateRate
			{
				// Server::broadcastRaw(Request::Update, &i.state);
				lastSend = Instant::now();
				Server::broadcastGameState(order);
				order += 1;
			}
		}
	}
}

fn main()
{
	Server::start();
}