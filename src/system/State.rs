use std::{collections::HashMap, net::IpAddr};

pub struct State
{
	playersList: HashMap<IpAddr, (String, String)>,
	pub checkpoint: String
}

impl State
{
	fn new() -> Self
	{
		Self
		{
			playersList: HashMap::new(),
			checkpoint: String::new()
		}
	}
	fn load(file: String) -> Self
	{
		let doc = json::parse(&file);
		if doc.is_err() { println!("Failed to load save."); return Self::new(); }
		let doc = doc.unwrap();
		let mut state = Self::new();

		for section in doc.entries()
		{
			if section.0 == "players"
			{
				for (ip, player) in section.1.entries()
				{
					let mut name = String::new();
					let mut class = String::new();
					for arg in player.entries()
					{
						if arg.0 == "name"
						{
							name = arg.1.as_str().unwrap_or("").to_string();
						}
						if arg.0 == "class"
						{
							class = arg.1.as_str().unwrap_or("").to_string();
						}
					}

					state.playersList.insert(
						ip.parse().unwrap(),
						(name, class)
					);
				}
			}
			if section.0 == "checkpoint"
			{
				state.checkpoint = section.1.as_str().unwrap_or("").to_string();
			}
		}
		
		state
	}

	pub fn init() -> Self
	{
		match std::fs::read_to_string("res/system/save.json")
		{
			Ok(file) => Self::load(file),
			Err(_) => Self::new()
		}
	}

	pub fn save(&self, checkpoint: String)
	{
		let mut players = json::JsonValue::new_object();
		for (ip, data) in &self.playersList
		{
			let mut info = json::JsonValue::new_object();
			let name = data.0.clone();
			let _ = info.insert("name", name.clone());
			let _ = info.insert("class", data.1.clone());
			let _ = players.insert(&ip.to_string(), info);
		}

		let mut state = json::JsonValue::new_object();
		let _ = state.insert("players", players);
		let _ = state.insert("checkpoint", checkpoint);

		let _ = std::fs::write(
			"res/system/save.json",
			json::stringify_pretty(state, 4)
		);
	}

	pub fn getPlayerInfo(&mut self, ip: IpAddr) -> (String, String)
	{
		match self.playersList.get(&ip)
		{
			Some(data) => data.clone(),
			None => (String::from("noname"), String::from("unknown"))
		}
	}
	
	pub fn setPlayerInfo(&mut self, ip: IpAddr, name: String, class: String)
	{
		self.playersList.insert(ip, (name, class));
	}
}