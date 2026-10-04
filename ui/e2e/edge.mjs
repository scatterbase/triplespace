/**
 * The edge for the end-to-end suite: the API's paths to the API, everything else to the web
 * replicas in turn (round robin), as Caddy or nginx would from `triplespace-web routes`
 * (ADR 0057 §3). Each response from a replica carries `X-Replica`, its index, so a test can
 * see that consecutive requests went to different replicas.
 *
 * node edge.mjs <listen port> <routes.json> <api origin> <web origin> [<web origin> …]
 */
import http from 'node:http';
import { readFileSync } from 'node:fs';

const [ port, routesFile, api, ...web ] = process.argv.slice( 2 );
// The file is the one run.sh wrote from `triplespace-web routes --format json`.
// eslint-disable-next-line security/detect-non-literal-fs-filename
const routes = JSON.parse( readFileSync( routesFile, 'utf8' ) ).api;
let next = 0;

function isApi( path ) {
	return routes.some( ( r ) => ( r.match === 'exact' ? path === r.path : path.startsWith( r.path ) ) );
}

http.createServer( ( req, res ) => {
	const path = new URL( req.url, 'http://edge' ).pathname;
	let origin = api;
	let replica = null;
	if ( !isApi( path ) ) {
		replica = next;
		origin = web[ next ];
		next = ( next + 1 ) % web.length;
	}
	const target = new URL( req.url, origin );
	const headers = { ...req.headers };
	const peer = req.socket.remoteAddress.replace( /^::ffff:/, '' );
	headers[ 'x-forwarded-for' ] = headers[ 'x-forwarded-for' ] ? `${ headers[ 'x-forwarded-for' ] }, ${ peer }` : peer;
	headers[ 'x-forwarded-proto' ] = 'http';
	const upstream = http.request( target, { method: req.method, headers }, ( up ) => {
		const out = { ...up.headers };
		if ( replica !== null ) {
			out[ 'x-replica' ] = String( replica );
		}
		res.writeHead( up.statusCode, out );
		up.pipe( res );
	} );
	upstream.on( 'error', ( e ) => {
		res.writeHead( 502, { 'content-type': 'text/plain' } );
		res.end( `edge: ${ e.message }\n` );
	} );
	req.pipe( upstream );
} ).listen( Number( port ), '127.0.0.1' );
