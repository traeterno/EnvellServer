use std::{io::{ErrorKind, Read, Write}, net::TcpStream};

use crate::system::Server::Server;

use super::Transmission::{ServerMessage, WebRequest, WebResponse};

pub struct WebClient
{
	tcp: Option<TcpStream>,
	req: Option<String>
}

impl WebClient
{
	pub fn new() -> Self
	{
		Self { tcp: None, req: None }
	}
	
	pub fn connect(&mut self, tcp: TcpStream)
	{
		let _ = tcp.set_nonblocking(true);
		self.tcp = Some(tcp);
	}

	pub fn disconnect(&mut self)
	{
		self.tcp = None;
	}

	pub fn update(&mut self) -> Option<ServerMessage>
	{
		if self.tcp.is_none() { return None; }
		let tcp = self.tcp.as_mut().unwrap();
		let buffer = &mut [0u8; 1024];
		match tcp.read(buffer)
		{
			Ok(size) =>
			{
				if size == 0 { return Some(ServerMessage::Disconnected); }
				self.req = Some(String::from_utf8_lossy(&buffer[0..size]).to_string());
			},
			Err(_) => {}
		}

		self.getRequest()
	}

	fn getRequest(&mut self) -> Option<ServerMessage>
	{
		if self.req.is_none() { return None; }
		let req = self.req.clone().unwrap();
		self.req = None;
		let req = WebRequest::build(req);
		match req
		{
			WebRequest::Invalid => None,
			WebRequest::Get(data) => self.get(data),
			WebRequest::Post(data) => self.post(data)
		}
	}

	fn get(&mut self, data: String) -> Option<ServerMessage>
	{
		let data = data.split("?").collect::<Vec<&str>>()[0];
		println!("Handling GET: {data}");
		if data == "/"
		{
			WebClient::sendResponse(
				WebResponse::MovedPermanently(String::from("/index.html")),
			);
		}
		else
		{
			let path = String::from("res/web") + &data;
			WebClient::sendResponse(
				match std::fs::read_to_string(path.clone())
				{
					Ok(text) =>
					{
						WebResponse::Ok(text, match path.split(".").last().unwrap()
						{
							"js" => String::from("text/javascript"),
							s => String::from("text/") + s
						})
					},
					Err(x) => match x.kind()
					{
						ErrorKind::InvalidData => match std::fs::read(path.clone())
						{
							Ok(data) =>
							{
								WebResponse::OkRaw(data, match path.split(".").last().unwrap()
								{
									"png" => String::from("image/png"),
									"otf" => String::from("application/x-font-opentype"),
									s => { println!("Unknown file: {s}"); String::from(s) }
								})
							},
							Err(x) => { println!("{x:#?}"); WebResponse::NotFound }
						},
						_ => { println!("{x:#?}"); WebResponse::NotFound }
					}
				}
			);
		}
		None
	}

	fn post(&mut self, data: String) -> Option<ServerMessage>
	{
		println!("Handling POST: {data}");
		match json::parse(&data)
		{
			Ok(parsed) => {
				let (cmd, data) = parsed.entries().nth(0).unwrap();
				WebClient::parsePost(cmd.to_string(), data.clone())
			},
			Err(_) => None
		}
	}

	fn parsePost(cmd: String, data: json::JsonValue) -> Option<ServerMessage>
	{
		if !data.is_object()
		{
			println!("Wrong request: arguments should be provided as object with properties.");
			return None;
		}

		if cmd == "players" { return Some(ServerMessage::PlayersList); }
		else if cmd == "chat"
		{
			for (id, value) in data.entries()
			{
				if id == "msg"
				{
					return Some(ServerMessage::Chat(value.as_str().unwrap_or("").to_string()));
				}
			}
			return None;
		}
		else if cmd == "getChat" { return Some(ServerMessage::ChatHistory); }
		else if cmd == "state" { return Some(ServerMessage::GameState); }
		else
		{
			println!("Unknown command: {cmd}");
			return Some(ServerMessage::Invalid);
		}
	}

	pub fn sendResponse(code: WebResponse)
	{
		std::thread::spawn(||
		{
			let c = Server::getInstance().getWebClient();
			if c.tcp.is_none()
			{
				println!("Tried to send response to empty TCP stream");
				return;
			}
			let tcp = c.tcp.as_mut().unwrap();
			let msg = code.build();
			println!("Sending {} bytes to web client", msg.len());
			match tcp.write_all(&msg)
			{
				Ok(_) => {},
				Err(x) => { println!("Error occured when sending response: {x:?}"); }
			}
		});
	}
}