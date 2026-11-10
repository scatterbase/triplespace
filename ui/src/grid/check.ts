/**
 * The page's check of the table (21 §6.6, §6.7), asked for as JSON: each row's status and
 * problems, what each column holds, and the text as the check wrote it back, a matched
 * row's `id` and a proposed data type filled in.
 */

/** A column as the check read it. */
export interface CheckColumn {
	kind: string;
	label: string | null;
	datatype: string | null;
}

/** A row as the check found it. */
export interface CheckRow {
	line: string;
	shown: string;
	status: string;
	kind: string;
	item: [ string, string ] | null;
	summary: string;
	errors: string[];
}

/** The check's answer. */
export interface Check {
	notices: { error: boolean; text: string }[];
	text: string;
	summary: string | null;
	confirm: string | null;
	columns: CheckColumn[] | null;
	labels: Record<string, string> | null;
	rows: CheckRow[] | null;
}

/** The fetch function; a test passes its own. */
export type Fetcher = ( input: string, init?: RequestInit ) => Promise<Response>;

/**
 * Posts the form's table to the page for checking.
 *
 * @param form The page's form
 * @param fetcher The fetch function; a test passes its own
 * @return The check
 */
export async function runCheck(
	form: HTMLFormElement,
	fetcher: Fetcher = ( i, init ) => fetch( i, init ),
): Promise<Check> {
	const body = new URLSearchParams();
	for ( const name of [ 'token', 'rows', 'shared', 'reference', 'batch', 'draft' ] ) {
		const field = form.elements.namedItem( name ) as
			HTMLInputElement | HTMLTextAreaElement | null;
		if ( field ) {
			body.set( name, field.value );
		}
	}
	body.set( 'wpStep', 'preview' );
	body.set( 'wpFormat', 'json' );
	const r = await fetcher( form.getAttribute( 'action' ) ?? '', {
		method: 'POST',
		credentials: 'same-origin',
		headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
		body: body.toString(),
	} );
	if ( !r.ok ) {
		throw new Error( `HTTP ${ r.status }` );
	}
	return await r.json() as Check;
}
