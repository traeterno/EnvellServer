#![allow(non_snake_case, static_mut_refs)]

mod system;
use system::Server::Server;

fn main()
{
	let server = Server::getInstance();

	server.debug(format!("Server is running. Waiting for players..."));

	loop
	{
		server.listen();
		server.update();
	}
}