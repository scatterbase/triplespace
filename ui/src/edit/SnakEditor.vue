<!-- eslint-disable vue/no-mutating-props -->
<template>
	<fieldset class="ts-snak-editor">
		<legend class="ts-snak-editor__legend">
			{{ legend }}
		</legend>
		<div v-if="kind === 'unsupported'" class="ts-snak-editor__note">
			{{ t( 'ts-edit-unsupported' ) }}
		</div>
		<template v-else>
			<CdxSelect
				v-model:selected="row.form.snaktype"
				class="ts-snak-editor__snaktype"
				:menu-items="snaktypes"
				:disabled="disabled"
				:aria-label="t( 'ts-edit-snaktype' )"
			/>
			<template v-if="row.form.snaktype === 'value'">
				<CdxTextInput
					v-if="kind === 'text'"
					v-model="row.form.text"
					:aria-label="legend"
					:disabled="disabled"
				/>
				<EntityLookup
					v-else-if="kind === 'entity'"
					:api="api"
					:lang="data.lang"
					:type="entityKind"
					:label="legend"
					:placeholder="t( 'ts-edit-search-entity' )"
					:initial-id="row.form.id"
					:initial-label="labels[ row.form.id ]"
					:disabled="disabled"
					@pick="pickValue"
				/>
				<template v-else-if="kind === 'monolingual'">
					<CdxTextInput
						v-model="row.form.text"
						:aria-label="legend"
						:lang="row.form.lang || undefined"
						:disabled="disabled"
					/>
					<CdxTextInput
						v-model="row.form.lang"
						class="ts-snak-editor__short"
						:aria-label="t( 'ts-edit-language' )"
						:placeholder="t( 'ts-edit-language' )"
						:disabled="disabled"
					/>
				</template>
				<template v-else-if="kind === 'quantity'">
					<CdxTextInput
						v-model="row.form.amount"
						class="ts-snak-editor__short"
						input-type="text"
						inputmode="decimal"
						:aria-label="`${ legend }: ${ t( 'ts-edit-amount' ) }`"
						:placeholder="t( 'ts-edit-amount' )"
						:disabled="disabled"
					/>
					<EntityLookup
						:api="api"
						:lang="data.lang"
						type="item"
						:label="`${ legend }: ${ t( 'ts-edit-unit' ) }`"
						:placeholder="t( 'ts-edit-unit' )"
						:initial-id="row.form.unit"
						:initial-label="labels[ row.form.unit ]"
						:disabled="disabled"
						@pick="pickUnit"
					/>
				</template>
				<template v-else-if="kind === 'time'">
					<CdxTextInput
						v-model="row.form.text"
						:aria-label="legend"
						:aria-description="t( 'ts-edit-time-help' )"
						:placeholder="t( 'ts-edit-time-placeholder' )"
						:disabled="disabled"
					/>
				</template>
				<template v-else-if="kind === 'coordinate'">
					<CdxTextInput
						v-model="row.form.lat"
						class="ts-snak-editor__short"
						inputmode="decimal"
						:aria-label="`${ legend }: ${ t( 'ts-edit-latitude' ) }`"
						:placeholder="t( 'ts-edit-latitude' )"
						:disabled="disabled"
					/>
					<CdxTextInput
						v-model="row.form.lon"
						class="ts-snak-editor__short"
						inputmode="decimal"
						:aria-label="`${ legend }: ${ t( 'ts-edit-longitude' ) }`"
						:placeholder="t( 'ts-edit-longitude' )"
						:disabled="disabled"
					/>
				</template>
			</template>
		</template>
		<slot />
	</fieldset>
</template>

<script setup lang="ts">
// One snak's editor: the kind of value (a value, unknown value, no value) and the
// fields of its data type. The row is the group editor's reactive state, which this
// component edits in place.
/* eslint-disable vue/no-mutating-props */
import { computed } from 'vue';
import { CdxSelect, CdxTextInput } from '@wikimedia/codex';
import type { Api } from './api';
import { msg } from './data';
import type { EditData } from './data';
import EntityLookup from './EntityLookup.vue';
import type { SnakRow } from './statements';
import { entityType, kindOf } from './values';

const props = defineProps<{
	row: SnakRow;
	legend: string;
	data: EditData;
	api: Api;
	labels: Record<string, string>;
	disabled?: boolean;
}>();

const t = ( key: string, ...params: string[] ): string => msg( props.data, key, ...params );
const kind = computed( () => kindOf( props.row.datatype ) );
const entityKind = computed( () => entityType( props.row.datatype ) );
const snaktypes = computed( () => [
	{ value: 'value', label: t( 'ts-edit-snaktype-value' ) },
	{ value: 'somevalue', label: t( 'ts-edit-snaktype-somevalue' ) },
	{ value: 'novalue', label: t( 'ts-edit-snaktype-novalue' ) }
] );

function pickValue( id: string, label: string ): void {
	props.row.form.id = id;
	props.labels[ id ] = label;
}

function pickUnit( id: string, label: string ): void {
	props.row.form.unit = id;
	props.labels[ id ] = label;
}
</script>
