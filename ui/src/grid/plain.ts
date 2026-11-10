/**
 * A cell's text as a `datavalue`, in the plain forms of 19 §3.1, as
 * `special/values.rs` reads them on the server: strings and IDs as written, a quantity
 * as a number, a time as a year, month or day, monolingual text as `text@lang`, a
 * coordinate as `lat, lon`. And back, for a cell that shows a value to be edited.
 */

import { EARTH, GREGORIAN, entityType, kindOf, parseTime, timeText } from '../edit/values';

/** A data value as Wikibase's JSON has it. */
export interface DataValue {
	type: string;
	value: unknown;
}

/**
 * The data value a cell's text is, for a property of a data type.
 *
 * @param datatype The property's data type
 * @param text The cell's text
 * @return The data value, or null when the text is not one of that type
 */
export function dataValueOf( datatype: string, text: string ): DataValue | null {
	const v = text.trim();
	if ( v === '' ) {
		return null;
	}
	switch ( kindOf( datatype ) ) {
		case 'text':
			return { type: 'string', value: v };
		case 'entity': {
			const type = entityType( datatype );
			const letter = type === 'property' ? 'P' : 'Q';
			if ( !new RegExp( `^[A-Za-z]{0,3}${ letter }\\d+$`, 'i' ).test( v ) ) {
				return null;
			}
			return { type: 'wikibase-entityid', value: { 'entity-type': type, id: v.toUpperCase() } };
		}
		case 'quantity': {
			const n = v.replace( /,/g, '' );
			if ( !/^[+-]?\d+(\.\d+)?$/.test( n ) ) {
				return null;
			}
			const amount = /^[+-]/.test( n ) ? n : `+${ n }`;
			return { type: 'quantity', value: { amount, unit: '1' } };
		}
		case 'time': {
			const t = parseTime( v );
			if ( !t ) {
				return null;
			}
			return { type: 'time', value: {
				time: t.time,
				timezone: 0,
				before: 0,
				after: 0,
				precision: t.precision,
				calendarmodel: GREGORIAN,
			} };
		}
		case 'monolingual': {
			const at = v.lastIndexOf( '@' );
			const lang = at >= 0 ? v.slice( at + 1 ).trim() : '';
			const words = at >= 0 ? v.slice( 0, at ).trim() : '';
			if ( words === '' || !/^[a-z]{2,8}(-[a-z0-9]{1,8})*$/.test( lang ) ) {
				return null;
			}
			return { type: 'monolingualtext', value: { text: words, language: lang } };
		}
		case 'coordinate': {
			const parts = v.split( ',' );
			const lat = Number( parts[ 0 ] );
			const lon = Number( parts[ 1 ] );
			if ( parts.length !== 2 || parts.some( ( p ) => p.trim() === '' ) ||
				Number.isNaN( lat ) || Number.isNaN( lon ) ||
				Math.abs( lat ) > 90 || Math.abs( lon ) > 180 ) {
				return null;
			}
			return { type: 'globecoordinate', value: { latitude: lat, longitude: lon, precision: 0.0001, globe: EARTH } };
		}
		default:
			return null;
	}
}

/**
 * A data value in the plain form `dataValueOf` reads.
 *
 * @param dv The data value
 * @return The text, or null for a value with no plain form
 */
export function plainOf( dv: DataValue | undefined ): string | null {
	if ( !dv ) {
		return null;
	}
	const v = dv.value as Record<string, unknown>;
	switch ( dv.type ) {
		case 'string':
			return String( dv.value );
		case 'wikibase-entityid':
			return typeof v.id === 'string' ? v.id : null;
		case 'quantity': {
			if ( ( v.unit !== undefined && v.unit !== '1' ) || v.upperBound !== undefined || v.lowerBound !== undefined ) {
				return null;
			}
			return String( v.amount ?? '' ).replace( /^\+/, '' );
		}
		case 'time': {
			const precision = Number( v.precision );
			if ( precision < 9 || precision > 11 ) {
				return null;
			}
			return timeText( String( v.time ), precision ).replace( /^(-?)0+(\d)/, '$1$2' );
		}
		case 'monolingualtext':
			return `${ String( v.text ) }@${ String( v.language ) }`;
		case 'globecoordinate':
			return `${ String( v.latitude ) }, ${ String( v.longitude ) }`;
		default:
			return null;
	}
}
