/**
 * Snaks as the statement editor holds them: a form of plain fields per data type, read
 * from a snak's Wikibase JSON and written back to it (ADR 0003 §8). A form the editor did
 * not change gives back the snak it came from, untouched, so a save changes only what
 * was edited.
 *
 * The site builds data values itself, since the API has no `wbparsevalue`: texts and
 * identifiers, entities, monolingual texts, quantities with a unit, times to the day,
 * month or year, and coordinates on Earth. Other data types are shown but not edited.
 */

/** A snak's Wikibase JSON. */
export interface Snak {
	snaktype: 'value' | 'somevalue' | 'novalue';
	property: string;
	datatype?: string;
	datavalue?: { type: string; value: unknown };
	hash?: string;
}

/** How a data type is edited. */
export type Kind = 'text' | 'entity' | 'monolingual' | 'quantity' | 'time' | 'coordinate' | 'unsupported';

/** The editor's fields for one snak. */
export interface Form {
	snaktype: Snak[ 'snaktype' ];
	/** A text, an identifier, a URL; a monolingual text's text; a time as `1952-03-11`. */
	text: string;
	/** A monolingual text's language. */
	lang: string;
	/** An entity value's ID. */
	id: string;
	/** A quantity's amount. */
	amount: string;
	/** A quantity's unit, as an entity ID, or empty for none. */
	unit: string;
	/** A coordinate. */
	lat: string;
	lon: string;
}

/** Wikibase's Gregorian calendar and Earth, which the API keeps as Wikidata's IRIs. */
export const GREGORIAN = 'http://www.wikidata.org/entity/Q1985727';
export const EARTH = 'http://www.wikidata.org/entity/Q2';

const TEXT_TYPES = [
	'string', 'external-id', 'url', 'commonsMedia', 'geo-shape', 'tabular-data', 'math',
	'musical-notation',
];

/**
 * How a data type is edited.
 *
 * @param datatype The property's data type
 * @return The kind of editor
 */
export function kindOf( datatype: string ): Kind {
	if ( TEXT_TYPES.includes( datatype ) ) {
		return 'text';
	}
	switch ( datatype ) {
		case 'wikibase-item':
		case 'wikibase-property':
			return 'entity';
		case 'monolingualtext':
			return 'monolingual';
		case 'quantity':
			return 'quantity';
		case 'time':
			return 'time';
		case 'globe-coordinate':
			return 'coordinate';
		default:
			return 'unsupported';
	}
}

/**
 * The entity type an entity data type's values have.
 *
 * @param datatype The data type
 * @return `item` or `property`
 */
export function entityType( datatype: string ): string {
	return datatype === 'wikibase-property' ? 'property' : 'item';
}

/**
 * A time value as the editor shows it: `1952-03-11`, `1952-03` or `1952`, by precision;
 * `-44` for 44 BCE.
 *
 * @param time The Wikibase time string, `+1952-03-11T00:00:00Z`
 * @param precision Its precision: 11 day, 10 month, 9 year
 * @return The text
 */
export function timeText( time: string, precision: number ): string {
	const m = /^([+-])(\d+)-(\d\d)-(\d\d)T/.exec( time );
	if ( !m ) {
		return time;
	}
	const year = `${ m[ 1 ] === '-' ? '-' : '' }${ m[ 2 ].replace( /^0+(?=\d{4})/, '' ) }`;
	if ( precision >= 11 ) {
		return `${ year }-${ m[ 3 ] }-${ m[ 4 ] }`;
	}
	if ( precision === 10 ) {
		return `${ year }-${ m[ 3 ] }`;
	}
	return year;
}

/**
 * A time as the editor's text writes it.
 *
 * @param text `1952-03-11`, `1952-03`, `1952` or `-44`
 * @return The Wikibase time string and precision, or null for text that is not one
 */
export function parseTime( text: string ): { time: string; precision: number } | null {
	const m = /^\s*(-?)(\d{1,16})(?:-(\d{1,2})(?:-(\d{1,2}))?)?\s*$/.exec( text );
	if ( !m ) {
		return null;
	}
	const month = m[ 3 ] ? Number( m[ 3 ] ) : 0;
	const day = m[ 4 ] ? Number( m[ 4 ] ) : 0;
	if ( month > 12 || day > 31 || ( m[ 3 ] && month === 0 ) || ( m[ 4 ] && day === 0 ) ) {
		return null;
	}
	const pad = ( n: number ): string => String( n ).padStart( 2, '0' );
	const year = m[ 2 ].padStart( 4, '0' );
	return {
		time: `${ m[ 1 ] === '-' ? '-' : '+' }${ year }-${ pad( month ) }-${ pad( day ) }T00:00:00Z`,
		precision: m[ 4 ] ? 11 : m[ 3 ] ? 10 : 9,
	};
}

/**
 * The entity ID a unit IRI names under the site's concept base; empty for none.
 *
 * @param unit The unit, `1` or an IRI
 * @param conceptBase The site's concept base
 * @return The ID, or the IRI where it is not the site's
 */
export function unitId( unit: string, conceptBase: string ): string {
	if ( unit === '1' || unit === '' ) {
		return '';
	}
	return conceptBase && unit.startsWith( conceptBase ) ? unit.slice( conceptBase.length ) : unit;
}

/**
 * An empty form.
 *
 * @return The form
 */
export function emptyForm(): Form {
	return { snaktype: 'value', text: '', lang: '', id: '', amount: '', unit: '', lat: '', lon: '' };
}

/**
 * The form of a snak.
 *
 * @param snak The snak
 * @param conceptBase The site's concept base, for units
 * @return The form
 */
export function formOf( snak: Snak, conceptBase: string ): Form {
	const f = emptyForm();
	f.snaktype = snak.snaktype;
	const v = snak.datavalue;
	if ( !v ) {
		return f;
	}
	const value = v.value as Record<string, unknown> & string;
	switch ( v.type ) {
		case 'string':
			f.text = String( v.value );
			break;
		case 'wikibase-entityid':
			f.id = String( value.id ?? '' );
			break;
		case 'monolingualtext':
			f.text = String( value.text ?? '' );
			f.lang = String( value.language ?? '' );
			break;
		case 'quantity':
			f.amount = String( value.amount ?? '' ).replace( /^\+/, '' );
			f.unit = unitId( String( value.unit ?? '1' ), conceptBase );
			break;
		case 'time':
			f.text = timeText( String( value.time ?? '' ), Number( value.precision ?? 11 ) );
			break;
		case 'globecoordinate':
			f.lat = String( value.latitude ?? '' );
			f.lon = String( value.longitude ?? '' );
			break;
	}
	return f;
}

function same( a: Form, b: Form ): boolean {
	return ( Object.keys( a ) as ( keyof Form )[] ).every( ( k ) => a[ k ] === b[ k ] );
}

/** A form the editor cannot make a snak of, with the message key that says why. */
export interface Invalid {
	problem: string;
}

/**
 * The snak a form describes: the original where the form is unchanged.
 *
 * @param form The form
 * @param property The property
 * @param datatype The property's data type
 * @param conceptBase The site's concept base, for units
 * @param original The snak the form came from, if any
 * @return The snak, or why there is none
 */
export function snakOf(
	form: Form,
	property: string,
	datatype: string,
	conceptBase: string,
	original?: Snak,
): Snak | Invalid {
	if ( original && same( form, formOf( original, conceptBase ) ) ) {
		return original;
	}
	if ( form.snaktype !== 'value' ) {
		return { snaktype: form.snaktype, property, datatype };
	}
	const snak = ( type: string, value: unknown ): Snak => (
		{ snaktype: 'value', property, datatype, datavalue: { type, value } }
	);
	switch ( kindOf( datatype ) ) {
		case 'text':
			return form.text.trim() ? snak( 'string', form.text.trim() ) : { problem: 'ts-edit-need-value' };
		case 'entity':
			return /^[A-Z]{0,3}[A-Z]\d+$/.test( form.id ) ?
				snak( 'wikibase-entityid', { 'entity-type': entityType( datatype ), id: form.id } ) :
				{ problem: 'ts-edit-need-entity' };
		case 'monolingual':
			return form.text.trim() && /^[a-z]{2,3}(-[a-z0-9]+)*$/i.test( form.lang.trim() ) ?
				snak( 'monolingualtext', { text: form.text.trim(), language: form.lang.trim().toLowerCase() } ) :
				{ problem: 'ts-edit-need-monolingual' };
		case 'quantity': {
			const amount = form.amount.trim();
			if ( !/^[+-]?\d+(\.\d+)?$/.test( amount ) ) {
				return { problem: 'ts-edit-need-number' };
			}
			const unit = form.unit === '' ? '1' :
				/^https?:/.test( form.unit ) ? form.unit : `${ conceptBase }${ form.unit }`;
			return snak( 'quantity', { amount: amount.startsWith( '-' ) || amount.startsWith( '+' ) ? amount : `+${ amount }`, unit } );
		}
		case 'time': {
			const t = parseTime( form.text );
			if ( !t ) {
				return { problem: 'ts-edit-need-time' };
			}
			const old = original?.datavalue?.value as Record<string, unknown> | undefined;
			return snak( 'time', {
				time: t.time,
				timezone: 0,
				before: 0,
				after: 0,
				precision: t.precision,
				calendarmodel: String( old?.calendarmodel ?? GREGORIAN ),
			} );
		}
		case 'coordinate': {
			const lat = Number( form.lat );
			const lon = Number( form.lon );
			if ( form.lat.trim() === '' || form.lon.trim() === '' || !Number.isFinite( lat ) || !Number.isFinite( lon ) ||
				Math.abs( lat ) > 90 || Math.abs( lon ) > 180 ) {
				return { problem: 'ts-edit-need-coordinate' };
			}
			const decimals = Math.max(
				( form.lat.split( '.' )[ 1 ] ?? '' ).length,
				( form.lon.split( '.' )[ 1 ] ?? '' ).length,
			);
			const old = original?.datavalue?.value as Record<string, unknown> | undefined;
			return snak( 'globecoordinate', {
				latitude: lat,
				longitude: lon,
				altitude: null,
				precision: Math.pow( 10, -Math.min( decimals, 9 ) ),
				globe: String( old?.globe ?? EARTH ),
			} );
		}
		default:
			return original ?? { problem: 'ts-edit-unsupported' };
	}
}

/**
 * Whether `snakOf` gave a problem rather than a snak.
 *
 * @param s Its answer
 * @return Whether it is a problem
 */
export function isInvalid( s: Snak | Invalid ): s is Invalid {
	return ( s as Invalid ).problem !== undefined;
}
