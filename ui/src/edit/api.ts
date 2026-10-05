/**
 * The browser's calls to the Action API, on the same origin with the viewer's cookie
 * (ADR 0057 §1): reads, and writes with the session's CSRF token, fetched once and
 * fetched again when the API says it went stale.
 */

/** A value the API takes as a parameter. */
export type Param = string | number | boolean | undefined;

/** An error the API answered with. */
export class ApiError extends Error {
	/** The error code: `editconflict`, `badtoken`, `permissiondenied`, … */
	public readonly code: string;

	/** The whole error object. */
	public readonly details: Record<string, unknown>;

	public constructor( code: string, info: string, details: Record<string, unknown> ) {
		super( info );
		this.code = code;
		this.details = details;
	}
}

/** The fetch function the client uses; a test passes its own. */
export type Fetcher = ( input: string, init?: RequestInit ) => Promise<Response>;

function encode( params: Record<string, Param> ): string {
	const out = new URLSearchParams();
	for ( const [ k, v ] of Object.entries( params ) ) {
		if ( v === undefined || v === false ) {
			continue;
		}
		out.set( k, v === true ? '1' : String( v ) );
	}
	out.set( 'format', 'json' );
	out.set( 'formatversion', '2' );
	return out.toString();
}

/** The client. */
export class Api {
	private token: string | null = null;

	public constructor(
		private readonly fetcher: Fetcher = ( i, init ) => fetch( i, init ),
		private readonly endpoint = '/w/api.php',
	) {}

	private async answer( r: Response ): Promise<Record<string, unknown>> {
		if ( !r.ok ) {
			throw new ApiError( 'http', `HTTP ${ r.status }`, {} );
		}
		const body = await r.json() as Record<string, unknown>;
		const error = body.error as Record<string, unknown> | undefined;
		if ( error ) {
			throw new ApiError(
				String( error.code || 'unknown' ),
				String( error.info || '' ),
				error,
			);
		}
		return body;
	}

	/**
	 * A `GET`.
	 *
	 * @param params The parameters
	 * @return The response body
	 */
	public async get( params: Record<string, Param> ): Promise<Record<string, unknown>> {
		const r = await this.fetcher( `${ this.endpoint }?${ encode( params ) }`, {
			credentials: 'same-origin',
			cache: 'no-cache',
		} );
		return this.answer( r );
	}

	/**
	 * The session's CSRF token.
	 *
	 * @param fresh Whether to ask again rather than reuse the one held
	 * @return The token
	 */
	public async csrf( fresh = false ): Promise<string> {
		if ( this.token === null || fresh ) {
			const body = await this.get( { action: 'query', meta: 'tokens', type: 'csrf' } );
			const query = body.query as { tokens: { csrftoken: string } };
			this.token = query.tokens.csrftoken;
		}
		return this.token;
	}

	/**
	 * A `POST` of a write, with the CSRF token; once more with a fresh token if the API
	 * says the one held is stale.
	 *
	 * @param params The parameters, without the token
	 * @return The response body
	 */
	public async write( params: Record<string, Param> ): Promise<Record<string, unknown>> {
		const send = async ( token: string ): Promise<Record<string, unknown>> => {
			const r = await this.fetcher( this.endpoint, {
				method: 'POST',
				credentials: 'same-origin',
				headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
				body: encode( { ...params, token } ),
			} );
			return this.answer( r );
		};
		try {
			return await send( await this.csrf() );
		} catch ( e ) {
			if ( e instanceof ApiError && e.code === 'badtoken' ) {
				return send( await this.csrf( true ) );
			}
			throw e;
		}
	}
}
